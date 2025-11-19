use std::{collections::HashMap, sync::Arc};

use anyhow::anyhow;
use thiserror::Error;
use uuid::Uuid;

mod outbox_dispatcher;
mod outbox_repository;

use crate::Peer;
pub use outbox_dispatcher::PeerMessagePayload;
use outbox_dispatcher::{PeerCommunicationOutboxDispatcher, PeerEnvelope};
use outbox_repository::{InMemoryOutboxRepository, OutboxRepository};

/// Trait for peer-to-peer communication.
///
/// Allows sending messages to other peers in the network using their peer IDs.
#[async_trait::async_trait]
pub trait PeerCommunication: Send + Sync {
    /// Send multiple messages to their respective peers.
    /// # Arguments
    /// * `messages` - A vector of messages to send, each containing the peer ID, process ID, and payload.
    /// # Errors
    /// * `PeerCommunicationError::PeerNotFound` - If any peer ID is not found or if there is an error sending any of the messages.
    /// * `PeerCommunicationError::OwnPeerId` - If attempting to send a message to own peer ID.
    /// * `PeerCommunicationError::Unknown` - For any other errors.
    async fn send_messages(&self, messages: Vec<PeerMessage>)
    -> Result<(), PeerCommunicationError>;
}

#[derive(Debug, Error)]
pub enum PeerCommunicationError {
    #[error("Peer ID {0} not found")]
    PeerNotFound(u8),
    #[error("Attempted to send message to own peer ID {0}")]
    OwnPeerId(u8),
    #[error(transparent)]
    Unknown(#[from] anyhow::Error),
}

pub struct PeerMessage {
    pub peer_id: u8,
    pub process_id: Uuid,
    pub payload: PeerMessagePayload,
}

impl PeerMessage {
    pub fn new_share_message(peer_id: u8, process_id: Uuid, value: u64) -> Self {
        Self {
            peer_id,
            process_id,
            payload: PeerMessagePayload::Share { value },
        }
    }

    pub fn new_shares_sum_message(peer_id: u8, process_id: Uuid, value: u64) -> Self {
        Self {
            peer_id,
            process_id,
            payload: PeerMessagePayload::SharesSum { value },
        }
    }
}

pub struct OutboxPeerCommunication {
    server_peer_id: u8,
    peer_urls: HashMap<u8, String>,
    outbox_repository: Arc<dyn OutboxRepository>,
}

impl OutboxPeerCommunication {
    pub fn new(
        server_peer_id: u8,
        peers: &[Peer],
        outbox_repository: Arc<dyn OutboxRepository>,
    ) -> Self {
        let peer_urls = peers
            .iter()
            .map(|p| (p.id, p.url.clone()))
            .collect::<HashMap<u8, String>>();

        Self {
            server_peer_id,
            peer_urls,
            outbox_repository,
        }
    }
}

#[async_trait::async_trait]
impl PeerCommunication for OutboxPeerCommunication {
    async fn send_messages(
        &self,
        messages: Vec<PeerMessage>,
    ) -> Result<(), PeerCommunicationError> {
        let envelopes = messages
            .into_iter()
            .map(|message| {
                if message.peer_id == self.server_peer_id {
                    return Err(PeerCommunicationError::OwnPeerId(message.peer_id));
                }
                let url = self
                    .peer_urls
                    .get(&message.peer_id)
                    .ok_or_else(|| PeerCommunicationError::PeerNotFound(message.peer_id))?;
                Ok(PeerEnvelope {
                    peer_id: message.peer_id,
                    peer_url: url.clone(),
                    process_id: message.process_id,
                    payload: message.payload,
                })
            })
            .collect::<Result<Vec<PeerEnvelope>, PeerCommunicationError>>()?;
        self.outbox_repository
            .enqueue_envelopes(envelopes)
            .await
            .map_err(|e| anyhow!(e).context("enqueuing messages to outbox repository"))?;

        Ok(())
    }
}

pub fn setup_peer_communication(
    server_peer_id: u8,
    peers: &[Peer],
) -> (
    OutboxPeerCommunication,
    PeerCommunicationOutboxDispatcher,
    IntervalPing,
) {
    let (tx, rx) = tokio::sync::mpsc::channel::<()>(100);

    let repository = Arc::new(InMemoryOutboxRepository::new(tx.clone()));
    let peer_communication =
        OutboxPeerCommunication::new(server_peer_id, peers, repository.clone());
    let outbox_dispatcher =
        PeerCommunicationOutboxDispatcher::new(repository, rx, 10, server_peer_id);
    let outbox_interval_ping = IntervalPing::new(tx);
    (peer_communication, outbox_dispatcher, outbox_interval_ping)
}

pub struct IntervalPing {
    channel_sender: tokio::sync::mpsc::Sender<()>,
}
impl IntervalPing {
    pub fn new(channel_sender: tokio::sync::mpsc::Sender<()>) -> Self {
        Self { channel_sender }
    }

    pub async fn run(&self) -> Result<(), anyhow::Error> {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            if let Err(e) = self.channel_sender.send(()).await {
                tracing::error!("Error sending ping to outbox dispatcher: {}", e);
            }
        }
    }
}
