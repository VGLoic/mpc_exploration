use std::{collections::HashMap, sync::RwLock};

use anyhow::anyhow;
use uuid::Uuid;

use crate::Peer;

#[async_trait::async_trait]
pub trait AdditionRepository: Send + Sync {
    async fn get_process(&self, process_id: Uuid) -> Result<AdditionProcess, anyhow::Error>;

    async fn create_process(&self, process_id: Uuid) -> Result<AdditionProcess, anyhow::Error>;

    async fn receive_new_process_share(
        &self,
        process_id: Uuid,
        share: Share,
    ) -> Result<AdditionProcess, anyhow::Error>;

    async fn receive_share(
        &self,
        process_id: Uuid,
        share: Share,
    ) -> Result<AdditionProcess, anyhow::Error>;

    async fn receive_sum_share(
        &self,
        process_id: Uuid,
        sum_share: Share,
    ) -> Result<AdditionProcess, anyhow::Error>;

    async fn get_completed_sums(&self) -> Result<Vec<u32>, anyhow::Error>;

    async fn delete_process(&self, process_id: Uuid) -> Result<(), anyhow::Error>;
}

pub struct InMemoryAdditionRepository {
    processes: RwLock<HashMap<Uuid, AdditionProcess>>,
    peer_ids: Vec<u8>,
    completed_sums: RwLock<Vec<u32>>,
}

impl InMemoryAdditionRepository {
    pub fn new(peers: &[Peer]) -> Self {
        Self {
            processes: RwLock::new(HashMap::new()),
            peer_ids: peers.iter().map(|p| p.id).collect(),
            completed_sums: RwLock::new(Vec::new()),
        }
    }
}

#[derive(Clone)]
pub struct AdditionProcess {
    pub input: u32,
    pub own_input_shares: Vec<Share>,
    pub peer_shares: HashMap<u8, u32>,
    pub peer_sum_shares: HashMap<u8, u32>,
    pub state: AdditionProcessState,
}

#[derive(Clone)]
pub enum AdditionProcessState {
    AwaitingPeerShares,
    AwaitingSumShares(AwaitingSumSharesState),
    Completed(CompletedAdditionProcess),
}

#[derive(Clone)]
pub struct AwaitingSumSharesState {
    pub own_sum_share: u32,
}

#[derive(Clone)]
pub struct CompletedAdditionProcess {
    pub own_sum_share: u32,
    pub final_sum: u32,
}

#[derive(Clone)]
pub struct Share {
    pub peer_id: u8,
    pub share: u32,
}

impl AdditionProcess {
    fn new(input: u32, peer_ids: &[u8]) -> Self {
        let own_input_shares = peer_ids
            .iter()
            .map(|&peer_id| {
                // For simplicity, using the same input as share. TODO: implement proper secret sharing.
                let share = input;
                Share { peer_id, share }
            })
            .collect::<Vec<_>>();
        Self {
            input,
            own_input_shares,
            peer_shares: HashMap::new(),
            peer_sum_shares: HashMap::new(),
            state: AdditionProcessState::AwaitingPeerShares,
        }
    }
}

#[async_trait::async_trait]
impl AdditionRepository for InMemoryAdditionRepository {
    async fn create_process(&self, process_id: Uuid) -> Result<AdditionProcess, anyhow::Error> {
        let input = rand::random::<u16>().into();
        let process = AdditionProcess::new(input, &self.peer_ids);
        let mut processes = self.processes.write().map_err(|e| {
            anyhow!("{e}").context("failed to acquire write lock on process creation")
        })?;
        processes.insert(process_id, process.clone());
        Ok(process)
    }

    async fn get_process(&self, process_id: Uuid) -> Result<AdditionProcess, anyhow::Error> {
        let processes = self.processes.read().map_err(|e| {
            anyhow!("{e}").context("failed to acquire read lock on getting process")
        })?;
        let process = processes
            .get(&process_id)
            .ok_or_else(|| anyhow!("process not found"))?;
        Ok(process.clone())
    }

    async fn receive_new_process_share(
        &self,
        process_id: Uuid,
        share: Share,
    ) -> Result<AdditionProcess, anyhow::Error> {
        let mut processes = self.processes.write().map_err(|e| {
            anyhow!("{e}").context("failed to acquire write lock on receiving new process share")
        })?;

        let input = rand::random::<u16>().into();
        let own_input_shares = self
            .peer_ids
            .iter()
            .map(|&peer_id| {
                // For simplicity, using the same input as share. TODO: implement proper secret sharing.
                let share = input;
                Share { peer_id, share }
            })
            .collect::<Vec<_>>();

        let mut peer_shares = HashMap::new();
        peer_shares.insert(share.peer_id, share.share);
        let process = AdditionProcess {
            input,
            own_input_shares,
            peer_shares,
            peer_sum_shares: HashMap::new(),
            state: AdditionProcessState::AwaitingPeerShares,
        };
        processes.insert(process_id, process.clone());

        Ok(process)
    }

    async fn receive_share(
        &self,
        process_id: Uuid,
        share: Share,
    ) -> Result<AdditionProcess, anyhow::Error> {
        let mut processes = self.processes.write().map_err(|e| {
            anyhow!("{e}").context("failed to acquire write lock on receiving share")
        })?;

        let process = processes
            .get_mut(&process_id)
            .ok_or(anyhow!("process not found"))?;
        match &mut process.state {
            AdditionProcessState::AwaitingPeerShares => {
                process.peer_shares.insert(share.peer_id, share.share);
                if process.peer_shares.len() == self.peer_ids.len() {
                    let own_sum_share = process.input + process.peer_shares.values().sum::<u32>();
                    process.state =
                        AdditionProcessState::AwaitingSumShares(AwaitingSumSharesState {
                            own_sum_share,
                        });
                }
            }
            _ => {
                return Err(anyhow!("invalid state for receiving share"));
            }
        }
        Ok(process.clone())
    }

    async fn receive_sum_share(
        &self,
        process_id: Uuid,
        sum_share: Share,
    ) -> Result<AdditionProcess, anyhow::Error> {
        let mut processes = self.processes.write().map_err(|e| {
            anyhow!("{e}").context("failed to acquire write lock on receiving sum share")
        })?;
        let process = processes
            .get_mut(&process_id)
            .ok_or_else(|| anyhow!("process not found"))?;
        match &mut process.state {
            AdditionProcessState::AwaitingSumShares(state) => {
                process
                    .peer_sum_shares
                    .insert(sum_share.peer_id, sum_share.share);
                if process.peer_sum_shares.len() == self.peer_ids.len() {
                    let final_sum: u32 =
                        state.own_sum_share + process.peer_sum_shares.values().sum::<u32>();
                    process.state = AdditionProcessState::Completed(CompletedAdditionProcess {
                        own_sum_share: state.own_sum_share,
                        final_sum,
                    });
                    let mut completed_sums = self.completed_sums.write().map_err(|e| {
                        anyhow!("{e}").context("failed to acquire write lock on completed sums")
                    })?;
                    completed_sums.push(final_sum);
                }
            }
            _ => {
                return Err(anyhow!("invalid state for receiving sum share"));
            }
        }
        Ok(process.clone())
    }

    async fn get_completed_sums(&self) -> Result<Vec<u32>, anyhow::Error> {
        let completed_sums = self.completed_sums.read().map_err(|e| {
            anyhow!("{e}").context("failed to acquire read lock on getting completed sums")
        })?;
        Ok(completed_sums.clone())
    }

    async fn delete_process(&self, process_id: Uuid) -> Result<(), anyhow::Error> {
        let mut processes = self.processes.write().map_err(|e| {
            anyhow!("{e}").context("failed to acquire write lock on process deletion")
        })?;
        processes.remove(&process_id);
        Ok(())
    }
}
