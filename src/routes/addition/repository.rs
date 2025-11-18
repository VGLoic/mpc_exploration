use std::{collections::HashMap, sync::RwLock};

use anyhow::anyhow;
use uuid::Uuid;

use crate::{
    Peer,
    mpc::{Share, recover_secret, split_secret},
};

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

    async fn receive_new_process_shares_sum(
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
    server_peer_id: u8,
    peer_ids: Vec<u8>,
}

impl InMemoryAdditionRepository {
    pub fn new(peers: &[Peer], server_peer_id: u8) -> Self {
        Self {
            processes: RwLock::new(HashMap::new()),
            server_peer_id,
            peer_ids: peers.iter().map(|p| p.id).collect(),
        }
    }

    pub fn all_ids(&self) -> Vec<u8> {
        let mut ids = self.peer_ids.clone();
        ids.push(self.server_peer_id);
        ids
    }
}

const PRIME: u64 = 1_000_000_007;

#[derive(Clone)]
pub struct AdditionProcess {
    pub input: u64,
    pub input_shares: HashMap<u8, u64>,
    pub peer_shares: HashMap<u8, u64>,
    pub peer_shares_sums: HashMap<u8, u64>,
    pub state: AdditionProcessState,
}

#[derive(Clone)]
pub enum AdditionProcessState {
    AwaitingPeerShares,
    AwaitingPeerSharesSum { shares_sum: u64 },
    Completed { final_sum: u64 },
}

#[derive(Clone)]
pub struct ReceivedShare {
    pub peer_id: u8,
    pub value: u64,
}

#[async_trait::async_trait]
impl AdditionRepository for InMemoryAdditionRepository {
    async fn create_process(&self, process_id: Uuid) -> Result<AdditionProcess, anyhow::Error> {
        let input = rand::random::<u16>().into();
        let input_shares = split_secret(input, &self.all_ids(), PRIME);
        let process = AdditionProcess {
            input,
            input_shares,
            peer_shares: HashMap::new(),
            peer_shares_sums: HashMap::new(),
            state: AdditionProcessState::AwaitingPeerShares,
        };
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
        let input_shares = split_secret(input, &self.all_ids(), PRIME);

        let mut peer_shares = HashMap::new();
        peer_shares.insert(from_peer_id, value);
        let process = AdditionProcess {
            input,
            input_shares,
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
                    let own_share = process
                        .input_shares
                        .get(&self.server_peer_id)
                        .ok_or(anyhow!("server peer share not found"))?;
                    let shares_sum: u64 = process
                        .peer_shares
                        .values()
                        .map(|&v| v as u128)
                        .sum::<u128>()
                        .wrapping_add(*own_share as u128)
                        .rem_euclid(PRIME as u128) as u64;
                    process.state = AdditionProcessState::AwaitingPeerSharesSum { shares_sum };
                }
            }
            _ => {
                return Err(anyhow!("invalid state for receiving share"));
            }
        }
        Ok(process.clone())
    }

    async fn receive_new_process_shares_sum(
        &self,
        process_id: Uuid,
        from_peer_id: u8,
        value: u64,
    ) -> Result<AdditionProcess, anyhow::Error> {
        let mut processes = self.processes.write().map_err(|e| {
            anyhow!("{e}").context("failed to acquire write lock on receiving sum share")
        })?;

        let input = rand::random::<u16>().into();
        let input_shares = split_secret(input, &self.all_ids(), PRIME);

        let mut peer_shares_sums = HashMap::new();
        peer_shares_sums.insert(from_peer_id, value);
        let process = AdditionProcess {
            input,
            input_shares,
            peer_shares: HashMap::new(),
            peer_shares_sums,
            state: AdditionProcessState::AwaitingPeerShares,
        };
        processes.insert(process_id, process.clone());

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
        process.peer_shares_sums.insert(from_peer_id, value);

        if let AdditionProcessState::AwaitingPeerSharesSum { shares_sum } = &process.state
            && process.peer_shares_sums.len() == self.peer_ids.len()
        {
            let mut all_shares_sums = vec![Share {
                point: self.server_peer_id,
                value: *shares_sum,
            }];
            for (i, v) in process.peer_shares_sums.iter() {
                all_shares_sums.push(Share {
                    point: *i,
                    value: *v,
                });
            }
            let final_sum = recover_secret(&all_shares_sums, PRIME)?;
            process.state = AdditionProcessState::Completed { final_sum };
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
