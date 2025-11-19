use std::collections::HashMap;

use anyhow::anyhow;
use futures::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::Peer;

/// Trait for peer-to-peer communication.
///
/// Allows sending messages to other peers in the network using their peer IDs.
#[async_trait::async_trait]
pub trait PeerCommunication: Send + Sync {
    /// Send a message to a specific peer.
    /// # Arguments
    /// * `message` - The message to send, containing the peer ID, process ID, and payload.
    /// # Errors
    /// * `PeerCommunicationError::PeerNotFound` - If the peer ID is not found or if there is an error sending the message.
    /// * `PeerCommunicationError::OwnPeerId` - If attempting to send a message to own peer ID.
    /// * `PeerCommunicationError::Unknown` - For any other errors.
    async fn send_message(&self, message: PeerMessage) -> Result<(), PeerCommunicationError>;

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

pub struct PeerEnvelope {
    pub peer_id: u8,
    pub peer_url: String,
    pub process_id: Uuid,
    pub payload: PeerMessagePayload,
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

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum PeerMessagePayload {
    Share { value: u64 },
    SharesSum { value: u64 },
}

pub struct HttpPeerCommunication {
    server_peer_id: u8,
    peer_urls: HashMap<u8, String>,
    tx: mpsc::Sender<PeerEnvelope>,
}

pub struct PeerCommunicationManager {
    server_peer_id: u8,
    rx: mpsc::Receiver<PeerEnvelope>,
    client: reqwest::Client,
}

pub fn setup_peer_communication(
    server_peer_id: u8,
    peers: &[Peer],
) -> (HttpPeerCommunication, PeerCommunicationManager) {
    let (tx, rx) = mpsc::channel::<PeerEnvelope>(32);

    let peer_urls = peers
        .iter()
        .map(|p| (p.id, p.url.clone()))
        .collect::<HashMap<u8, String>>();

    let http_peer_communication = HttpPeerCommunication {
        server_peer_id,
        peer_urls,
        tx,
    };

    let peer_communication_manager = PeerCommunicationManager {
        rx,
        server_peer_id,
        client: reqwest::Client::new(),
    };

    (http_peer_communication, peer_communication_manager)
}

impl PeerCommunicationManager {
    pub async fn run(&mut self) -> Result<(), anyhow::Error> {
        while let Some(message) = self.rx.recv().await {
            tracing::info!(
                "Sending message to peer {} for process {}",
                message.peer_id,
                message.process_id
            );

            let response = self
                .client
                .post(format!(
                    "{}/additions/{}/receive",
                    message.peer_url, message.process_id
                ))
                .header("X-PEER-ID", self.server_peer_id.to_string())
                .json(&message.payload)
                .send()
                .await
                .map_err(|e| anyhow!("{e}").context("sending message to peer"))?;

            if !response.status().is_success() {
                tracing::error!(
                    "Failed to send message to peer {}: HTTP {}",
                    message.peer_id,
                    response.status()
                );
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl PeerCommunication for HttpPeerCommunication {
    async fn send_message(&self, message: PeerMessage) -> Result<(), PeerCommunicationError> {
        if message.peer_id == self.server_peer_id {
            return Err(PeerCommunicationError::OwnPeerId(message.peer_id));
        }

        let url = self
            .peer_urls
            .get(&message.peer_id)
            .ok_or_else(|| PeerCommunicationError::PeerNotFound(message.peer_id))?;

        let message = PeerEnvelope {
            peer_id: message.peer_id,
            peer_url: url.clone(),
            process_id: message.process_id,
            payload: message.payload,
        };

        self.tx
            .send(message)
            .await
            .map_err(|e| anyhow!(e).context("sending message to peer communication channel"))?;

        Ok(())
    }

    async fn send_messages(
        &self,
        messages: Vec<PeerMessage>,
    ) -> Result<(), PeerCommunicationError> {
        let bodies = stream::iter(messages)
            .map(|message| async move { self.send_message(message).await })
            .buffer_unordered(2);

        let results: Vec<Result<(), PeerCommunicationError>> = bodies.collect().await;
        for result in results {
            result?;
        }

        Ok(())
    }
}
