use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::anyhow;
use futures::{StreamExt, stream};

use crate::{
    Peer,
    domains::additions::{AwaitingPeerSharesProcess, AwaitingPeerSharesSumProcess},
    peer_communication::peer_client::{AdditionProcessProgress, PeerClient},
};

use super::{
    AdditionProcess, ReceiveSharesRequest, ReceiveSharesRequestError, ReceiveSharesSumsRequest,
    ReceiveSharesSumsRequestError, notifier::IntervalPing, repository::AdditionProcessRepository,
};

pub fn setup_addition_process_orchestrator(
    repository: Arc<dyn AdditionProcessRepository>,
    peer_client: Arc<dyn PeerClient>,
    own_peer_id: u8,
    peers: &[Peer],
) -> (AdditionProcessOrchestrator, IntervalPing) {
    let (channel_sender, channel_receiver) = tokio::sync::mpsc::channel::<()>(1);
    let orchestrator = AdditionProcessOrchestrator::new(
        repository,
        own_peer_id,
        peers,
        peer_client,
        channel_receiver,
    );
    let interval_ping = IntervalPing::new(channel_sender);
    (orchestrator, interval_ping)
}

/// Orchestrates the addition processes by interacting with the repository and the peers.
pub struct AdditionProcessOrchestrator {
    repository: Arc<dyn AdditionProcessRepository>,
    own_peer_id: u8,
    peer_ids: HashSet<u8>,
    channel_receiver: tokio::sync::mpsc::Receiver<()>,
    peer_client: Arc<dyn PeerClient>,
    failures_attempts: HashMap<uuid::Uuid, u8>,
}

impl AdditionProcessOrchestrator {
    pub fn new(
        repository: Arc<dyn AdditionProcessRepository>,
        own_peer_id: u8,
        peers: &[Peer],
        peer_client: Arc<dyn PeerClient>,
        channel_receiver: tokio::sync::mpsc::Receiver<()>,
    ) -> Self {
        let peer_ids = peers.iter().map(|peer| peer.id).collect::<HashSet<u8>>();
        Self {
            repository,
            own_peer_id,
            peer_ids,
            channel_receiver,
            peer_client,
            failures_attempts: HashMap::new(),
        }
    }

    pub async fn run(&mut self) {
        while self.channel_receiver.recv().await.is_some() {
            let processes = match self.repository.get_ongoing_processes().await {
                Ok(processes) => processes
                    .into_iter()
                    .filter(|p| {
                        if let Some(attempts) = self.failures_attempts.get(&p.id()) {
                            *attempts < 5
                        } else {
                            true
                        }
                    })
                    .collect::<Vec<AdditionProcess>>(),
                Err(e) => {
                    tracing::error!("Failed to fetch ongoing addition processes: {:?}", e);
                    continue;
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

            let mut failure_ids = vec![];
            for process in processes {
                if let Err(e) = self.poll_and_update_process(&process).await {
                    tracing::error!(
                        "Failed to poll and update process {}: {:?}",
                        process.id(),
                        e
                    );
                    failure_ids.push(process.id());
                }
            }
            if !failure_ids.is_empty() {
                for failure_id in &failure_ids {
                    let counter = self.failures_attempts.entry(*failure_id).or_insert(0);
                    *counter += 1;
                    if *counter >= 5 {
                        tracing::error!(
                            "Process {} reached maximum failure attempts. It will be skipped in future orchestrations.",
                            failure_id
                        );
                    }
                }
            }
        }
    }

    async fn poll_and_update_process(
        &self,
        process: &AdditionProcess,
    ) -> Result<(), anyhow::Error> {
        match process {
            AdditionProcess::AwaitingPeerShares(p) => {
                tracing::info!("Polling for peer shares for process {}", p.id);
                self.poll_for_peer_shares(p).await
            }
            AdditionProcess::AwaitingPeerSharesSum(p) => {
                tracing::info!("Polling for peer shares sums for process {}", p.id);
                self.poll_for_peer_shares_sums(p).await
            }
            AdditionProcess::Completed(_p) => {
                // No action needed for completed processes
                Ok(())
            }
        }
    }

    /// Looks for missing shares from peers and tries to fetch them.
    /// Once shares are fetched, create the associated request and use the repository to update the process state accordingly.
    async fn poll_for_peer_shares(
        &self,
        process: &AwaitingPeerSharesProcess,
    ) -> Result<(), anyhow::Error> {
        let missing_peer_ids = self
            .peer_ids
            .iter()
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
            self.peer_ids.len(),
        )
        .map_err(|e| match e {
            ReceiveSharesRequestError::Unknown(e) => e.context("creating receive shares request"),
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
        process: &AwaitingPeerSharesSumProcess,
    ) -> Result<(), anyhow::Error> {
        let missing_peer_ids = self
            .peer_ids
            .iter()
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
            self.peer_ids.len(),
        )
        .map_err(|e| match e {
            ReceiveSharesSumsRequestError::Unknown(e) => {
                e.context("creating receive shares sums request")
            }
        })?;
        let updated_process = self
            .repository
            .receive_shares_sums(receive_shares_sums_request)
            .await
            .map_err(|e| e.context("updating process with received shares sums"))?;

        if let AdditionProcess::Completed(completed_process) = updated_process {
            tracing::info!(
                "Process {} completed with final sum: {}",
                process.id,
                completed_process.final_sum
            );
        }

        Ok(())
    }

    async fn fetch_process_progress_from_peers(
        &self,
        peer_ids: Vec<u8>,
        process_id: uuid::Uuid,
    ) -> Result<Vec<AdditionProcessProgressFromPeer>, anyhow::Error> {
        let bodies = stream::iter(peer_ids)
            .map(|peer_id| async move {
                self.peer_client
                    .fetch_process_progress(peer_id, process_id)
                    .await
                    .map(|progress| AdditionProcessProgressFromPeer { peer_id, progress })
            })
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
}

struct AdditionProcessProgressFromPeer {
    peer_id: u8,
    progress: AdditionProcessProgress,
}
