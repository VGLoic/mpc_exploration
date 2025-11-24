use std::sync::Arc;

mod outbox_relayer;
mod outbox_repository;
mod outbox_sender;
pub mod peer_client;
mod peer_messages;

use crate::Peer;
use outbox_relayer::OutboxPeerMessagesRelayer;
pub use outbox_relayer::PeerMessagePayload;
use outbox_repository::InMemoryOutboxRepository;
use outbox_sender::OutboxPeerMessagesSender;

pub use outbox_sender::PeerMessagesSender;
use peer_client::HttpPeerClient;
pub use peer_messages::PeerMessage;

pub fn setup_peer_communication(
    server_peer_id: u8,
    peers: &[Peer],
) -> (
    Arc<HttpPeerClient>,
    OutboxPeerMessagesSender,
    OutboxPeerMessagesRelayer,
    IntervalPing,
) {
    let peer_client = Arc::new(peer_client::HttpPeerClient::new(server_peer_id, peers));

    let (tx, rx) = tokio::sync::mpsc::channel::<()>(100);

    let repository = Arc::new(InMemoryOutboxRepository::new(tx.clone()));
    let messages_sender = OutboxPeerMessagesSender::new(server_peer_id, repository.clone());
    let messages_relayer = OutboxPeerMessagesRelayer::new(repository, rx, 10, peer_client.clone());
    let relayer_pinger = IntervalPing::new(tx);
    (
        peer_client,
        messages_sender,
        messages_relayer,
        relayer_pinger,
    )
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
                tracing::error!("Error sending ping to sender channel: {}", e);
            }
        }
    }
}
