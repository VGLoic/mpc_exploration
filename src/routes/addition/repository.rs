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
        from_peer_id: u8,
        value: u64,
    ) -> Result<AdditionProcess, anyhow::Error>;

    async fn receive_share(
        &self,
        process_id: Uuid,
        from_peer_id: u8,
        value: u64,
    ) -> Result<AdditionProcess, anyhow::Error>;

    async fn receive_shares_sum(
        &self,
        process_id: Uuid,
        from_peer_id: u8,
        value: u64,
    ) -> Result<AdditionProcess, anyhow::Error>;

    async fn delete_process(&self, process_id: Uuid) -> Result<(), anyhow::Error>;
}

pub struct InMemoryAdditionRepository {
    processes: RwLock<HashMap<Uuid, AdditionProcess>>,
    peer_ids: Vec<u8>,
}

impl InMemoryAdditionRepository {
    pub fn new(peers: &[Peer]) -> Self {
        Self {
            processes: RwLock::new(HashMap::new()),
            peer_ids: peers.iter().map(|p| p.id).collect(),
        }
    }
}

#[derive(Clone)]
pub struct AdditionProcess {
    pub input: u64,
    pub peer_shares: HashMap<u8, u64>,
    pub peer_shares_sums: HashMap<u8, u64>,
    pub state: AdditionProcessState,
}

#[derive(Clone)]
pub enum AdditionProcessState {
    AwaitingPeerShares,
    AwaitingSumShares,
    Completed,
}

#[derive(Clone)]
pub struct ReceivedShare {
    pub peer_id: u8,
    pub value: u64,
}

impl AdditionProcess {
    fn new(input: u64) -> Self {
        Self {
            input,
            peer_shares: HashMap::new(),
            peer_shares_sums: HashMap::new(),
            state: AdditionProcessState::AwaitingPeerShares,
        }
    }
}

#[async_trait::async_trait]
impl AdditionRepository for InMemoryAdditionRepository {
    async fn create_process(&self, process_id: Uuid) -> Result<AdditionProcess, anyhow::Error> {
        let input = rand::random::<u16>().into();
        let process = AdditionProcess::new(input);
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
        from_peer_id: u8,
        value: u64,
    ) -> Result<AdditionProcess, anyhow::Error> {
        let mut processes = self.processes.write().map_err(|e| {
            anyhow!("{e}").context("failed to acquire write lock on receiving new process share")
        })?;

        let input = rand::random::<u16>().into();
        let mut peer_shares = HashMap::new();
        peer_shares.insert(from_peer_id, value);
        let process = AdditionProcess {
            input,
            peer_shares,
            peer_shares_sums: HashMap::new(),
            state: AdditionProcessState::AwaitingPeerShares,
        };
        processes.insert(process_id, process.clone());

        Ok(process)
    }

    async fn receive_share(
        &self,
        process_id: Uuid,
        from_peer_id: u8,
        value: u64,
    ) -> Result<AdditionProcess, anyhow::Error> {
        let mut processes = self.processes.write().map_err(|e| {
            anyhow!("{e}").context("failed to acquire write lock on receiving share")
        })?;

        let process = processes
            .get_mut(&process_id)
            .ok_or(anyhow!("process not found"))?;
        match &mut process.state {
            AdditionProcessState::AwaitingPeerShares => {
                process.peer_shares.insert(from_peer_id, value);
                if process.peer_shares.len() == self.peer_ids.len() {
                    process.state = AdditionProcessState::AwaitingSumShares;
                }
            }
            _ => {
                return Err(anyhow!("invalid state for receiving share"));
            }
        }
        Ok(process.clone())
    }

    async fn receive_shares_sum(
        &self,
        process_id: Uuid,
        from_peer_id: u8,
        value: u64,
    ) -> Result<AdditionProcess, anyhow::Error> {
        let mut processes = self.processes.write().map_err(|e| {
            anyhow!("{e}").context("failed to acquire write lock on receiving sum share")
        })?;
        let process = processes
            .get_mut(&process_id)
            .ok_or_else(|| anyhow!("process not found"))?;
        match &mut process.state {
            AdditionProcessState::AwaitingSumShares => {
                process.peer_shares_sums.insert(from_peer_id, value);
                if process.peer_shares_sums.len() == self.peer_ids.len() {
                    process.state = AdditionProcessState::Completed;
                }
            }
            _ => {
                return Err(anyhow!("invalid state for receiving sum share"));
            }
        }
        Ok(process.clone())
    }

    async fn delete_process(&self, process_id: Uuid) -> Result<(), anyhow::Error> {
        let mut processes = self.processes.write().map_err(|e| {
            anyhow!("{e}").context("failed to acquire write lock on process deletion")
        })?;
        processes.remove(&process_id);
        Ok(())
    }
}
