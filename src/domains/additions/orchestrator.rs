use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::anyhow;
use futures::{StreamExt, stream};

use crate::Peer;

use super::{
    AdditionProcess, AdditionProcessProgress, ReceiveSharesRequest, ReceiveSharesRequestError,
    ReceiveSharesSumsRequest, ReceiveSharesSumsRequestError, repository::AdditionProcessRepository,
};

pub fn setup_addition_process_orchestrator(
    repository: Arc<dyn AdditionProcessRepository>,
    own_peer_id: u8,
    peers: &[Peer],
) -> (AdditionProcessOrchestrator, IntervalPing) {
    let (channel_sender, channel_receiver) = tokio::sync::mpsc::channel::<()>(100);
    let orchestrator =
        AdditionProcessOrchestrator::new(repository, own_peer_id, peers, channel_receiver);
    let interval_ping = IntervalPing::new(channel_sender);
    (orchestrator, interval_ping)
}

/// Orchestrates the addition processes by interacting with the repository and the peers.
pub struct AdditionProcessOrchestrator {
    repository: Arc<dyn AdditionProcessRepository>,
    own_peer_id: u8,
    peer_urls: HashMap<u8, String>,
    channel_receiver: tokio::sync::mpsc::Receiver<()>,
    client: reqwest::Client,
    failures_attempts: Mutex<HashMap<uuid::Uuid, u8>>,
}

impl AdditionProcessOrchestrator {
    pub fn new(
        repository: Arc<dyn AdditionProcessRepository>,
        own_peer_id: u8,
        peers: &[Peer],
        channel_receiver: tokio::sync::mpsc::Receiver<()>,
    ) -> Self {
        let peer_urls = peers
            .iter()
            .filter(|peer| peer.id != own_peer_id)
            .map(|peer| (peer.id, peer.url.clone()))
            .collect::<HashMap<u8, String>>();
        Self {
            repository,
            own_peer_id,
            peer_urls,
            channel_receiver,
            client: reqwest::Client::new(),
            failures_attempts: Mutex::new(HashMap::new()),
        }
    }

    pub async fn start(&mut self) {
        while self.channel_receiver.recv().await.is_some() {
            let raw_processes = match self.repository.get_ongoing_processes().await {
                Ok(processes) => processes,
                Err(e) => {
                    tracing::error!("Failed to fetch ongoing addition processes: {:?}", e);
                    continue;
                }
            };
            let processes = {
                if let Ok(failures_attempts) = self.failures_attempts.lock() {
                    raw_processes
                        .into_iter()
                        .filter(|process| {
                            if let Some(attempts) = failures_attempts.get(&process.id) {
                                *attempts < 5
                            } else {
                                true
                            }
                        })
                        .collect::<Vec<AdditionProcess>>()
                } else {
                    tracing::error!("Failed to acquire lock on failures_attempts");
                    raw_processes
                }
            };

            if processes.is_empty() {
                tracing::info!("no ongoing addition processes to orchestrate.");
            } else {
                tracing::info!(
                    "Orchestrating {} ongoing addition processes.",
                    processes.len()
                );
            }

            for process in processes {
                let mut failure_ids = vec![];
                if let Err(e) = self.poll_and_update_process(&process).await {
                    tracing::error!("Failed to poll and update process {}: {:?}", process.id, e);
                    failure_ids.push(process.id);
                }
                if !failure_ids.is_empty() {
                    if let Ok(mut failures_attempts) = self.failures_attempts.lock() {
                        for failure_id in &failure_ids {
                            let counter = failures_attempts.entry(*failure_id).or_insert(0);
                            *counter += 1;
                            if *counter >= 5 {
                                tracing::error!(
                                    "Process {} reached maximum failure attempts. It will be skipped in future orchestrations.",
                                    failure_id
                                );
                            }
                        }
                    } else {
                        tracing::error!("Failed to acquire lock on failures_attempts");
                    }
                }
            }
        }
    }

    async fn poll_and_update_process(
        &self,
        process: &AdditionProcess,
    ) -> Result<(), anyhow::Error> {
        match process.state {
            super::AdditionProcessState::AwaitingPeerShares => {
                tracing::info!("Polling for peer shares for process {}", process.id);
                self.poll_for_peer_shares(process).await
            }
            super::AdditionProcessState::AwaitingPeerSharesSum { .. } => {
                tracing::info!("Polling for peer shares sums for process {}", process.id);
                self.poll_for_peer_shares_sums(process).await
            }
            super::AdditionProcessState::Completed { .. } => {
                // No action needed for completed processes
                Ok(())
            }
        }
    }

    /// Looks for missing shares from peers and tries to fetch them.
    /// Once shares are fetched, create the associated request and use the repository to update the process state accordingly.
    async fn poll_for_peer_shares(&self, process: &AdditionProcess) -> Result<(), anyhow::Error> {
        let missing_peer_ids = self
            .peer_urls
            .keys()
            .filter(|peer_id| !process.received_shares.contains_key(peer_id))
            .cloned()
            .collect::<Vec<u8>>();
        if missing_peer_ids.is_empty() {
            return Err(anyhow!("unexpected: no missing peer shares to poll for"));
        }
        let peer_progresses = self
            .fetch_process_progress_from_peers(missing_peer_ids, process.id)
            .await
            .map_err(|e| e.context("fetching missing process progresses"))?;
        let received_shares = peer_progresses
            .into_iter()
            .map(|progress| (progress.peer_id, progress.progress.share))
            .collect::<HashMap<u8, u64>>();

        let receive_shares_request = ReceiveSharesRequest::new(
            process,
            received_shares,
            self.peer_urls.len(),
        )
        .map_err(|e| match e {
            ReceiveSharesRequestError::Unknown(e) => e.context("creating receive shares request"),
            ReceiveSharesRequestError::InvalidState => {
                anyhow!("invalid process state when creating receive shares request")
            }
        })?;
        self.repository
            .receive_shares(receive_shares_request)
            .await
            .map_err(|e| e.context("updating process with received shares"))?;

        Ok(())
    }

    /// Looks for missing shares sums from peers and tries to fetch them.
    /// Once shares sums are fetched, create the associated request and use the repository to update the process state accordingly.
    async fn poll_for_peer_shares_sums(
        &self,
        process: &AdditionProcess,
    ) -> Result<(), anyhow::Error> {
        let missing_peer_ids = self
            .peer_urls
            .keys()
            .filter(|peer_id| !process.received_shares_sums.contains_key(peer_id))
            .cloned()
            .collect::<Vec<u8>>();
        if missing_peer_ids.is_empty() {
            return Err(anyhow!(
                "unexpected: no missing peer shares sums to poll for"
            ));
        }
        let peer_progresses = self
            .fetch_process_progress_from_peers(missing_peer_ids, process.id)
            .await
            .map_err(|e| e.context("fetching missing process progresses for shares sums"))?;
        let received_shares_sums = peer_progresses
            .into_iter()
            .filter_map(|progress_from_peer| {
                if let Some(shares_sum) = progress_from_peer.progress.shares_sum {
                    Some((progress_from_peer.peer_id, shares_sum))
                } else {
                    None
                }
            })
            .collect::<HashMap<u8, u64>>();

        let receive_shares_sums_request = ReceiveSharesSumsRequest::new(
            process,
            received_shares_sums,
            self.own_peer_id,
            self.peer_urls.len(),
        )
        .map_err(|e| match e {
            ReceiveSharesSumsRequestError::Unknown(e) => {
                e.context("creating receive shares sums request")
            }
            ReceiveSharesSumsRequestError::InvalidState => {
                anyhow!("invalid process state when creating receive shares sums request")
            }
        })?;
        self.repository
            .receive_shares_sums(receive_shares_sums_request)
            .await
            .map_err(|e| e.context("updating process with received shares sums"))?;

        // Implement the logic to poll for peer shares sums and update the process state accordingly.
        Ok(())
    }

    async fn fetch_process_progress_from_peers(
        &self,
        peer_ids: Vec<u8>,
        process_id: uuid::Uuid,
    ) -> Result<Vec<AdditionProcessProgressFromPeer>, anyhow::Error> {
        let bodies = stream::iter(peer_ids)
            .map(|peer_id| async move { self.fetch_progress_from_peer(peer_id, process_id).await })
            .buffer_unordered(5);
        let results: Vec<Result<AdditionProcessProgressFromPeer, anyhow::Error>> =
            bodies.collect().await;
        let mut progresses = Vec::new();
        for result in results {
            match result {
                Ok(progress) => progresses.push(progress),
                Err(e) => tracing::error!("Error fetching process progress from peer: {}", e),
            }
        }
        if progresses.is_empty() {
            return Err(anyhow!("Failed to fetch progress from any peer"));
        }
        Ok(progresses)
    }

    async fn fetch_progress_from_peer(
        &self,
        peer_id: u8,
        process_id: uuid::Uuid,
    ) -> Result<AdditionProcessProgressFromPeer, anyhow::Error> {
        let peer_url = self
            .peer_urls
            .get(&peer_id)
            .ok_or(anyhow!("URL for peer {peer_id} not found"))?;

        self.client
            .get(format!("{}/additions/{}/progress", peer_url, process_id))
            .header("X-PEER-ID", self.own_peer_id.to_string())
            .send()
            .await
            .map_err(|e| anyhow!(e).context("sending request to peer"))?
            .json::<AdditionProcessProgress>()
            .await
            .map_err(|e| anyhow!(e).context("parsing response from peer"))
            .map(|progress| AdditionProcessProgressFromPeer { peer_id, progress })
    }
}

struct AdditionProcessProgressFromPeer {
    peer_id: u8,
    progress: AdditionProcessProgress,
}

pub struct IntervalPing {
    channel_sender: tokio::sync::mpsc::Sender<()>,
}
impl IntervalPing {
    pub fn new(channel_sender: tokio::sync::mpsc::Sender<()>) -> Self {
        Self { channel_sender }
    }

    pub async fn run(&self) -> Result<(), anyhow::Error> {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
        loop {
            interval.tick().await;
            if let Err(e) = self.channel_sender.send(()).await {
                tracing::error!("Error sending ping to outbox dispatcher: {}", e);
            }
        }
    }
}
