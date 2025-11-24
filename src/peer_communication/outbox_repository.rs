use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use super::outbox_relayer::PeerEnvelope;
use anyhow::anyhow;
use uuid::Uuid;

/// Repository trait for managing outbox items to be sent to peers.
/// It supports enqueuing, dequeuing, re-enqueuing, and fetching items ready to send.
#[async_trait::async_trait]
pub trait OutboxRepository: Send + Sync {
    /// Enqueues multiple peer envelopes into the outbox.
    /// It pings the dispatcher channel after enqueuing.
    /// # Arguments
    /// * `envelopes` - A vector of `PeerEnvelope` items to enqueue.
    /// # Returns
    /// * A vector of `OutboxItem` representing the enqueued items.
    async fn enqueue_envelopes(
        &self,
        envelopes: Vec<PeerEnvelope>,
    ) -> Result<Vec<OutboxItem>, anyhow::Error>;

    /// Dequeues multiple outbox items by their IDs.
    /// # Arguments
    /// * `ids` - A slice of `Uuid` representing the IDs of the outbox items to dequeue.
    /// # Returns
    /// * A vector of `OutboxItem` representing the dequeued items.
    fn dequeue_envelopes(&self, ids: &[Uuid]) -> Result<Vec<OutboxItem>, anyhow::Error>;

    /// Re-enqueues multiple outbox items by their IDs with a specified delay.
    /// # Arguments
    /// * `ids` - A slice of `Uuid` representing the IDs of the outbox items to re-enqueue.
    /// * `delay` - A `std::time::Duration` specifying the delay before the items are scheduled to be sent again.
    /// # Returns
    /// * An empty result indicating success or failure.
    fn re_enqueue_envelopes(
        &self,
        ids: &[Uuid],
        delay: std::time::Duration,
    ) -> Result<(), anyhow::Error>;

    /// Retrieves a list of outbox items that are ready to be sent, up to a specified limit.
    /// # Arguments
    /// * `limit` - The maximum number of outbox items to retrieve.
    /// # Returns
    /// * A vector of `OutboxItem` representing the items ready to be sent.
    fn get_items_ready_to_send(&self, limit: usize) -> Result<Vec<OutboxItem>, anyhow::Error>;
}

#[derive(Clone)]
pub struct OutboxItem {
    pub id: Uuid,
    pub envelope: PeerEnvelope,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub scheduled_at: chrono::DateTime<chrono::Utc>,
    pub attempts: u8,
}

pub struct InMemoryOutboxRepository {
    items: Arc<Mutex<HashMap<Uuid, OutboxItem>>>,
    channel_sender: tokio::sync::mpsc::Sender<()>,
}

impl InMemoryOutboxRepository {
    pub fn new(sender: tokio::sync::mpsc::Sender<()>) -> Self {
        Self {
            items: Arc::new(Mutex::new(HashMap::new())),
            channel_sender: sender,
        }
    }
}

#[async_trait::async_trait]
impl OutboxRepository for InMemoryOutboxRepository {
    async fn enqueue_envelopes(
        &self,
        envelopes: Vec<PeerEnvelope>,
    ) -> Result<Vec<OutboxItem>, anyhow::Error> {
        let items = {
            let mut items = Vec::new();
            let mut items_lock = self.items.lock().map_err(|e| {
                anyhow!("{e}").context("failed to lock envelopes mutex while enquing multiple")
            })?;
            for envelope in envelopes {
                let item = OutboxItem {
                    id: Uuid::new_v4(),
                    envelope,
                    created_at: chrono::Utc::now(),
                    scheduled_at: chrono::Utc::now(),
                    attempts: 0,
                };
                items_lock.insert(item.id, item.clone());
                items.push(item);
            }
            items
        };

        let _ = self.channel_sender.send(()).await;

        Ok(items)
    }

    fn re_enqueue_envelopes(
        &self,
        ids: &[Uuid],
        delay: std::time::Duration,
    ) -> Result<(), anyhow::Error> {
        let mut items_lock = self.items.lock().map_err(|e| {
            anyhow!("{e}").context("failed to lock envelopes mutex while re-enquing")
        })?;
        let now = chrono::Utc::now();
        for id in ids {
            let item = items_lock.get_mut(id).ok_or_else(|| {
                anyhow!("Outbox item with id {id} not found").context("re-enqueueing envelopes")
            })?;
            item.attempts += 1;
            item.scheduled_at = now
                + chrono::Duration::from_std(delay).map_err(|e| {
                    anyhow!("{e}").context("converting std::time::Duration to chrono::Duration")
                })?;
        }
        Ok(())
    }

    fn dequeue_envelopes(&self, ids: &[Uuid]) -> Result<Vec<OutboxItem>, anyhow::Error> {
        let mut items = Vec::new();
        let mut items_lock = self
            .items
            .lock()
            .map_err(|e| anyhow!("{e}").context("failed to lock envelopes mutex while dequeing"))?;
        for id in ids {
            if let Some(item) = items_lock.remove(id) {
                items.push(item);
            }
        }
        Ok(items)
    }

    fn get_items_ready_to_send(&self, limit: usize) -> Result<Vec<OutboxItem>, anyhow::Error> {
        let items_lock = self.items.lock().map_err(|e| {
            anyhow!("{e}").context("failed to lock envelopes mutex while getting ready to send")
        })?;
        let now = chrono::Utc::now();
        let mut ready_items: Vec<OutboxItem> = items_lock
            .values()
            .filter(|item| item.scheduled_at <= now)
            .cloned()
            .collect();
        ready_items.sort_by_key(|item| item.scheduled_at);
        Ok(ready_items.into_iter().take(limit).collect())
    }
}
