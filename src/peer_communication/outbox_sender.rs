use std::{collections::HashMap, sync::Arc};

use anyhow::anyhow;
use thiserror::Error;
use uuid::Uuid;

use crate::Peer;

use super::outbox_relayer::{PeerEnvelope, PeerMessagePayload};
use super::outbox_repository::OutboxRepository;

/// Trait for peer-to-peer communication.
///
/// Allows sending messages to other peers in the network using their peer IDs.
#[async_trait::async_trait]
pub trait PeerMessagesSender: Send + Sync {
    /// Send multiple messages to their respective peers.
    /// # Arguments
    /// * `messages` - A vector of messages to send, each containing the peer ID, process ID, and payload.
    /// # Errors
    /// * `PeerMessagesSenderError::PeerNotFound` - If any peer ID is not found or if there is an error sending any of the messages.
    /// * `PeerMessagesSenderError::OwnPeerId` - If attempting to send a message to own peer ID.
    /// * `PeerMessagesSenderError::Unknown` - For any other errors.
    async fn send_messages(
        &self,
        messages: Vec<PeerMessage>,
    ) -> Result<(), PeerMessagesSenderError>;
}

#[derive(Debug, Error)]
pub enum PeerMessagesSenderError {
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
    pub fn new_process(peer_id: u8, process_id: Uuid) -> Self {
        Self {
            peer_id,
            process_id,
            payload: PeerMessagePayload::NewProcess {},
        }
    }
}

pub struct OutboxPeerMessagesSender {
    server_peer_id: u8,
    peer_urls: HashMap<u8, String>,
    outbox_repository: Arc<dyn OutboxRepository>,
}

impl OutboxPeerMessagesSender {
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
impl PeerMessagesSender for OutboxPeerMessagesSender {
    async fn send_messages(
        &self,
        messages: Vec<PeerMessage>,
    ) -> Result<(), PeerMessagesSenderError> {
        let envelopes = messages
            .into_iter()
            .map(|message| {
                if message.peer_id == self.server_peer_id {
                    return Err(PeerMessagesSenderError::OwnPeerId(message.peer_id));
                }
                let url = self
                    .peer_urls
                    .get(&message.peer_id)
                    .ok_or_else(|| PeerMessagesSenderError::PeerNotFound(message.peer_id))?;
                Ok(PeerEnvelope {
                    peer_id: message.peer_id,
                    peer_url: url.clone(),
                    process_id: message.process_id,
                    payload: message.payload,
                })
            })
            .collect::<Result<Vec<PeerEnvelope>, PeerMessagesSenderError>>()?;
        self.outbox_repository
            .enqueue_envelopes(envelopes)
            .await
            .map_err(|e| anyhow!(e).context("enqueuing messages to outbox repository"))?;

        Ok(())
    }
}
