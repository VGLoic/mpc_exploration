use anyhow::anyhow;
use futures::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use super::outbox_repository::{OutboxItem, OutboxRepository};

/// Relayer for sending outbox items to their respective peers.
/// It listens for signals on a channel to trigger dispatching of outbox items.
pub struct OutboxPeerMessagesRelayer {
    /// Repository for managing outbox items.
    outbox_repository: Arc<dyn OutboxRepository>,
    /// Receiver channel to listen for dispatch signals.
    channel_receiver: tokio::sync::mpsc::Receiver<()>,
    /// Maximum number of items to process in one batch.
    batch_size: usize,
    /// The ID of the server peer.
    server_peer_id: u8,
    /// HTTP client for sending requests.
    client: reqwest::Client,
}

#[derive(Clone)]
pub struct PeerEnvelope {
    pub peer_id: u8,
    pub peer_url: String,
    pub process_id: Uuid,
    pub payload: PeerMessagePayload,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum PeerMessagePayload {
    NewProcess {},
}

impl OutboxPeerMessagesRelayer {
    pub fn new(
        outbox_repository: Arc<dyn OutboxRepository>,
        channel_receiver: tokio::sync::mpsc::Receiver<()>,
        batch_size: usize,
        server_peer_id: u8,
    ) -> Self {
        Self {
            outbox_repository,
            channel_receiver,
            batch_size,
            server_peer_id,
            client: reqwest::Client::new(),
        }
    }
}

impl OutboxPeerMessagesRelayer {
    /// Runs the relayer, continuously listening for signals to poll and dispatch outbox items.
    pub async fn run(&mut self) {
        while self.channel_receiver.recv().await.is_some() {
            if let Err(e) = self.poll_and_dispatch().await {
                tracing::error!("Error during poll and dispatch: {}", e);
            }
        }
    }

    /// Polls the outbox repository for items ready to send and dispatches them.
    async fn poll_and_dispatch(&self) -> Result<(), anyhow::Error> {
        let items = self
            .outbox_repository
            .get_items_ready_to_send(self.batch_size)
            .map_err(|e| e.context("poll and dispatch of outbox items"))?;

        let item_extracts = items
            .iter()
            .map(|item| (item.id, item.attempts))
            .collect::<Vec<(Uuid, u8)>>();

        let bodies = stream::iter(items)
            .map(|item| async move { self.dispatch(item).await })
            .buffer_unordered(5);
        let results: Vec<Result<(), anyhow::Error>> = bodies.collect().await;

        let mut success_ids = Vec::new();
        let mut to_be_retried_ids = Vec::new();
        let mut to_be_abandoned = Vec::new();
        for (index, result) in results.into_iter().enumerate() {
            match result {
                Ok(()) => success_ids.push(item_extracts[index].0),
                Err(_) => {
                    let attempts = item_extracts[index].1;
                    if attempts >= 5 {
                        to_be_abandoned.push(item_extracts[index].0);
                    } else {
                        to_be_retried_ids.push(item_extracts[index].0);
                    }
                }
            }
        }

        // Remove successfully sent items from outbox
        if !success_ids.is_empty() {
            self.outbox_repository
                .dequeue_envelopes(&success_ids)
                .map_err(|e| e.context("dequeue successfully sent outbox items"))?;
        }
        if !to_be_retried_ids.is_empty() {
            tracing::info!(
                "Outbox dispatch completed with {} failures, re-enqueuing failed items",
                to_be_retried_ids.len()
            );

            self.outbox_repository
                .re_enqueue_envelopes(&to_be_retried_ids, std::time::Duration::from_secs(1))
                .map_err(|e| e.context("re-enqueue failed outbox items"))?;
        }
        if !to_be_abandoned.is_empty() {
            tracing::warn!(
                "Outbox dispatch abandoning {} items after max attempts",
                to_be_abandoned.len()
            );
            self.outbox_repository
                .dequeue_envelopes(&to_be_abandoned)
                .map_err(|e| e.context("dequeue abandoned outbox items"))?;
        }

        Ok(())
    }

    /// Dispatches a single outbox item to its designated peer.
    /// The item is mapped to an HTTP POST request.
    async fn dispatch(&self, item: OutboxItem) -> Result<(), anyhow::Error> {
        let response = self
            .client
            .post(format!(
                "{}/additions/{}/initiate",
                item.envelope.peer_url, item.envelope.process_id
            ))
            .header("X-PEER-ID", self.server_peer_id.to_string())
            .json(&item.envelope.payload)
            .send()
            .await
            .map_err(|e| anyhow!("{e}").context("sending outbox item to peer"))?;
        if !response.status().is_success() {
            tracing::error!(
                "Failed to dispatch outbox item {}: HTTP {}",
                item.id,
                response.status()
            );
        }
        Ok(())
    }
}
