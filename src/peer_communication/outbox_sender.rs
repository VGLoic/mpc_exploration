use std::sync::Arc;

use super::{outbox_repository::OutboxRepository, peer_messages::PeerMessage};
use anyhow::anyhow;
use thiserror::Error;

/// Trait for peer-to-peer communication.
///
/// Allows sending messages to other peers in the network using their peer IDs.
#[async_trait::async_trait]
pub trait PeerMessagesSender: Send + Sync {
    /// Send multiple messages to their respective peers.
    /// # Arguments
    /// * `messages` - A vector of messages to send, each containing the peer ID, process ID, and payload.
    /// # Errors
    /// * `PeerMessagesSenderError::OwnPeerId` - If attempting to send a message to own peer ID.
    /// * `PeerMessagesSenderError::Unknown` - For any other errors.
    async fn send_messages(
        &self,
        messages: Vec<PeerMessage>,
    ) -> Result<(), PeerMessagesSenderError>;
}

#[derive(Debug, Error)]
pub enum PeerMessagesSenderError {
    #[error("Attempted to send message to own peer ID {0}")]
    OwnPeerId(u8),
    #[error(transparent)]
    Unknown(#[from] anyhow::Error),
}

pub struct OutboxPeerMessagesSender {
    server_peer_id: u8,
    outbox_repository: Arc<dyn OutboxRepository>,
}

impl OutboxPeerMessagesSender {
    pub fn new(server_peer_id: u8, outbox_repository: Arc<dyn OutboxRepository>) -> Self {
        Self {
            server_peer_id,
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
        if messages.is_empty() {
            return Ok(());
        }
        if messages.iter().any(|m| m.peer_id() == self.server_peer_id) {
            return Err(PeerMessagesSenderError::OwnPeerId(self.server_peer_id));
        }
        self.outbox_repository
            .enqueue_messages(messages)
            .await
            .map_err(|e| anyhow!(e).context("enqueuing messages to outbox repository"))?;

        Ok(())
    }
}
