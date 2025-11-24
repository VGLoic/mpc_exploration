use std::collections::HashMap;

use crate::domains::additions::{
    AwaitingPeerSharesProcess, AwaitingPeerSharesSumProcess, CompletedProcess,
};

use super::{
    AdditionProcess, CreateProcessRequest, ReceiveSharesRequest, ReceiveSharesSumsRequest,
};
use tokio::sync::RwLock;
use uuid::Uuid;

#[async_trait::async_trait]
pub trait AdditionProcessRepository: Send + Sync {
    /// Retrieves an addition process by its ID.
    /// # Arguments
    /// * `process_id` - The UUID of the addition process to retrieve.
    async fn get_process(&self, process_id: Uuid) -> Result<AdditionProcess, anyhow::Error>;

    /// Retrieves all ongoing addition processes.
    async fn get_ongoing_processes(&self) -> Result<Vec<AdditionProcess>, anyhow::Error>;

    /// Creates a new addition process.
    /// # Arguments
    /// * `request` - The request containing the details for the new addition process.
    async fn create_process(
        &self,
        request: CreateProcessRequest,
    ) -> Result<AdditionProcess, anyhow::Error>;

    /// Receives shares for an existing addition process.
    /// If a shares sum is provided, the process is updated to the next state.
    /// # Arguments
    /// * `request` - The request containing the shares to be received.
    async fn receive_shares(
        &self,
        request: ReceiveSharesRequest,
    ) -> Result<AdditionProcess, anyhow::Error>;

    /// Receives shares sums for an existing addition process.
    /// If the final sum is provided, the process is marked as completed.
    /// # Arguments
    /// * `request` - The request containing the shares sums to be received.
    async fn receive_shares_sums(
        &self,
        request: ReceiveSharesSumsRequest,
    ) -> Result<AdditionProcess, anyhow::Error>;

    /// Deletes an addition process by its ID.
    /// # Arguments
    /// * `process_id` - The UUID of the addition process to delete.
    async fn delete_process(&self, process_id: Uuid) -> Result<(), anyhow::Error>;
}

pub struct InMemoryAdditionProcessRepository {
    processes: RwLock<HashMap<Uuid, AdditionProcess>>,
}

impl InMemoryAdditionProcessRepository {
    pub fn new() -> Self {
        Self {
            processes: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryAdditionProcessRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl AdditionProcessRepository for InMemoryAdditionProcessRepository {
    async fn get_process(&self, process_id: Uuid) -> Result<AdditionProcess, anyhow::Error> {
        let processes = self.processes.read().await;
        processes
            .get(&process_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Process not found"))
    }

    async fn get_ongoing_processes(&self) -> Result<Vec<AdditionProcess>, anyhow::Error> {
        let processes = self.processes.read().await;
        let mut ongoing_processes = Vec::new();
        for process in processes.values() {
            if !matches!(process, AdditionProcess::Completed(_)) {
                ongoing_processes.push(process.clone());
            }
        }
        Ok(ongoing_processes)
    }

    async fn create_process(
        &self,
        request: CreateProcessRequest,
    ) -> Result<AdditionProcess, anyhow::Error> {
        let mut processes = self.processes.write().await;
        if processes.contains_key(&request.process_id) {
            return Err(anyhow::anyhow!("Process with this ID already exists"));
        }
        let process = AdditionProcess::AwaitingPeerShares(AwaitingPeerSharesProcess {
            id: request.process_id,
            input_shares: request.input_shares.clone(),
            received_shares: HashMap::new(),
        });
        processes.insert(request.process_id, process.clone());
        Ok(process)
    }

    async fn receive_shares(
        &self,
        request: ReceiveSharesRequest,
    ) -> Result<AdditionProcess, anyhow::Error> {
        let mut processes = self.processes.write().await;
        let process = processes
            .get_mut(&request.process_id)
            .ok_or_else(|| anyhow::anyhow!("Process not found"))?;

        let internal_process = match process {
            AdditionProcess::AwaitingPeerShares(p) => p,
            _ => {
                return Err(anyhow::anyhow!(
                    "Process is not in a state to receive shares"
                ));
            }
        };

        for (peer_id, share) in &request.received_shares {
            internal_process.received_shares.insert(*peer_id, *share);
        }

        if let Some(shares_sum) = request.computed_shares_sum {
            let internal_process = AwaitingPeerSharesSumProcess {
                id: internal_process.id,
                input_shares: internal_process.input_shares.clone(),
                received_shares: internal_process.received_shares.clone(),
                shares_sum,
                received_shares_sums: HashMap::new(),
            };
            *process = AdditionProcess::AwaitingPeerSharesSum(internal_process);
        }

        Ok(process.clone())
    }

    async fn receive_shares_sums(
        &self,
        request: ReceiveSharesSumsRequest,
    ) -> Result<AdditionProcess, anyhow::Error> {
        let mut processes = self.processes.write().await;
        let process = processes
            .get_mut(&request.process_id)
            .ok_or_else(|| anyhow::anyhow!("Process not found"))?;

        let internal_process = match process {
            AdditionProcess::AwaitingPeerSharesSum(p) => p,
            _ => {
                return Err(anyhow::anyhow!(
                    "Process is not in a state to receive shares sums"
                ));
            }
        };

        for (peer_id, share_sum) in &request.received_shares_sums {
            internal_process
                .received_shares_sums
                .insert(*peer_id, *share_sum);
        }

        if let Some(final_sum) = request.final_sum {
            let completed_process = CompletedProcess {
                id: internal_process.id,
                input_shares: internal_process.input_shares.clone(),
                received_shares: internal_process.received_shares.clone(),
                shares_sum: internal_process.shares_sum,
                received_shares_sums: internal_process.received_shares_sums.clone(),
                final_sum,
            };
            *process = AdditionProcess::Completed(completed_process);
        }

        Ok(process.clone())
    }

    async fn delete_process(&self, process_id: Uuid) -> Result<(), anyhow::Error> {
        let mut processes = self.processes.write().await;
        processes.remove(&process_id);
        Ok(())
    }
}
