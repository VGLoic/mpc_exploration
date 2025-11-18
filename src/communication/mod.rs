use std::collections::HashMap;

use anyhow::anyhow;
use futures::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::Peer;

/// Trait for peer-to-peer communication.
///
/// Allows sending messages to other peers in the network using their peer IDs.
#[async_trait::async_trait]
pub trait PeerCommunication: Send + Sync {
    /// Send a message to a specific peer.
    /// # Arguments
    /// * `peer_id` - The ID of the peer to send the message to.
    /// * `message` - The message to send.
    /// # Errors
    /// * `PeerCommunicationError::PeerNotFound` - If the peer ID is not found or if there is an error sending the message.
    /// * `PeerCommunicationError::OwnPeerId` - If attempting to send a message to own peer ID.
    /// * `PeerCommunicationError::Unknown` - For any other errors.
    async fn send_message(
        &self,
        peer_id: u8,
        message: PeerMessage,
    ) -> Result<(), PeerCommunicationError>;

    /// Send multiple messages to their respective peers.
    /// # Arguments
    /// * `messages` - A vector of tuples containing peer IDs and their corresponding messages.
    /// # Errors
    /// * `PeerCommunicationError::PeerNotFound` - If any peer ID is not found or if there is an error sending any of the messages.
    /// * `PeerCommunicationError::OwnPeerId` - If attempting to send a message to own peer ID.
    /// * `PeerCommunicationError::Unknown` - For any other errors.
    async fn send_messages(
        &self,
        messages: Vec<(u8, PeerMessage)>,
    ) -> Result<(), PeerCommunicationError>;
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
    pub process_id: Uuid,
    pub payload: PeerMessagePayload,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum PeerMessagePayload {
    Share { value: u64 },
    SharesSum { value: u64 },
}

impl PeerMessage {
    pub fn new_share_message(process_id: Uuid, value: u64) -> Self {
        Self {
            process_id,
            payload: PeerMessagePayload::Share { value },
        }
    }

    pub fn new_shares_sum_message(process_id: Uuid, value: u64) -> Self {
        Self {
            process_id,
            payload: PeerMessagePayload::SharesSum { value },
        }
    }
}

pub struct HttpPeerCommunication {
    server_peer_id: u8,
    peer_urls: HashMap<u8, String>,
    client: reqwest::Client,
}

impl HttpPeerCommunication {
    pub fn new(server_peer_id: u8, peers: &[Peer]) -> Self {
        let peer_urls = peers
            .iter()
            .map(|p| (p.id, p.url.clone()))
            .collect::<HashMap<u8, String>>();

        Self {
            server_peer_id,
            peer_urls,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl PeerCommunication for HttpPeerCommunication {
    async fn send_message(
        &self,
        peer_id: u8,
        message: PeerMessage,
    ) -> Result<(), PeerCommunicationError> {
        if peer_id == self.server_peer_id {
            return Err(PeerCommunicationError::OwnPeerId(peer_id));
        }
        let url = self
            .peer_urls
            .get(&peer_id)
            .ok_or_else(|| PeerCommunicationError::PeerNotFound(peer_id))?;

        let response = self
            .client
            .post(format!("{url}/additions/{}/receive", message.process_id))
            .header("X-PEER-ID", self.server_peer_id.to_string())
            .json(&message.payload)
            .send()
            .await
            .map_err(|e| anyhow!("{e}").context("sending message to peer"))?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(anyhow!(
                "Failed to send message to peer {}: HTTP {}",
                peer_id,
                response.status()
            )
            .into())
        }
    }

    async fn send_messages(
        &self,
        messages: Vec<(u8, PeerMessage)>,
    ) -> Result<(), PeerCommunicationError> {
        let bodies = stream::iter(messages)
            .map(|(peer_id, message)| async move { self.send_message(peer_id, message).await })
            .buffer_unordered(2);

        let results: Vec<Result<(), PeerCommunicationError>> = bodies.collect().await;
        for result in results {
            result?;
        }

        Ok(())
    }
}
