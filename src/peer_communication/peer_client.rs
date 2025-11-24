use std::collections::HashMap;

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Peer;

#[async_trait::async_trait]
pub trait PeerClient: Send + Sync {
    async fn notify_new_process(&self, peer_id: u8, process_id: Uuid) -> Result<(), anyhow::Error>;

    async fn fetch_process_progress(
        &self,
        peer_id: u8,
        process_id: Uuid,
    ) -> Result<AdditionProcessProgress, anyhow::Error>;
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AdditionProcessProgress {
    pub share: u64,
    pub shares_sum: Option<u64>,
}

pub struct HttpPeerClient {
    server_peer_id: u8,
    peer_urls: HashMap<u8, String>,
    client: reqwest::Client,
}

impl HttpPeerClient {
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
impl PeerClient for HttpPeerClient {
    async fn notify_new_process(&self, peer_id: u8, process_id: Uuid) -> Result<(), anyhow::Error> {
        let peer_url = self
            .peer_urls
            .get(&peer_id)
            .ok_or_else(|| anyhow!("Peer ID {} not found", peer_id))?;

        let response = self
            .client
            .post(format!("{}/additions/{}/initiate", peer_url, process_id))
            .header("X-PEER-ID", self.server_peer_id.to_string())
            .send()
            .await
            .map_err(|e| anyhow!("{e}").context("notifying peer of new process"))?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Failed to notify peer {}: HTTP {}",
                peer_id,
                response.status()
            ));
        }

        Ok(())
    }

    async fn fetch_process_progress(
        &self,
        peer_id: u8,
        process_id: Uuid,
    ) -> Result<AdditionProcessProgress, anyhow::Error> {
        let peer_url = self
            .peer_urls
            .get(&peer_id)
            .ok_or_else(|| anyhow!("Peer ID {} not found", peer_id))?;

        let response = self
            .client
            .get(format!("{}/additions/{}/progress", peer_url, process_id))
            .header("X-PEER-ID", self.server_peer_id.to_string())
            .send()
            .await
            .map_err(|e| anyhow!("{e}").context("fetching process progress from peer"))?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Failed to fetch process progress from peer {}: HTTP {}",
                peer_id,
                response.status()
            ));
        }

        let progress = response
            .json::<AdditionProcessProgress>()
            .await
            .map_err(|e| anyhow!("{e}").context("parsing process progress response"))?;

        Ok(progress)
    }
}
