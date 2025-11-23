use std::collections::HashMap;

use super::{
    AdditionProcess, AdditionProcessState, CreateProcessRequest, ReceiveSharesRequest,
    ReceiveSharesSumsRequest,
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
            if !matches!(process.state, AdditionProcessState::Completed { .. }) {
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
        let process = AdditionProcess {
            id: request.process_id,
            input: request.input,
            own_share: request.own_share,
            shares_to_send: request.shares_to_send,
            received_shares: HashMap::new(),
            received_shares_sums: HashMap::new(),
            state: AdditionProcessState::AwaitingPeerShares,
        };
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

        for (peer_id, share) in &request.received_shares {
            process.received_shares.insert(*peer_id, *share);
        }

        if let Some(shares_sum) = request.computed_shares_sum {
            process.state = AdditionProcessState::AwaitingPeerSharesSum { shares_sum };
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

        let shares_sum = match process.state {
            AdditionProcessState::AwaitingPeerSharesSum { shares_sum } => shares_sum,
            _ => {
                return Err(anyhow::anyhow!(
                    "Process is not in a state to receive shares sums"
                ));
            }
        };

        for (peer_id, share_sum) in &request.received_shares_sums {
            process.received_shares_sums.insert(*peer_id, *share_sum);
        }

        if let Some(final_sum) = request.final_sum {
            process.state = AdditionProcessState::Completed {
                shares_sum,
                final_sum,
            };
        }

        Ok(process.clone())
    }

    async fn delete_process(&self, process_id: Uuid) -> Result<(), anyhow::Error> {
        let mut processes = self.processes.write().await;
        processes.remove(&process_id);
        Ok(())
    }
}
