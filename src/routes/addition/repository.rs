use std::{collections::HashMap, sync::RwLock};

use anyhow::anyhow;
use uuid::Uuid;

use super::domain::{AdditionProcess, AdditionProcessState, CreateProcessRequest};
use crate::{
    Peer,
    mpc::{Share, recover_secret, split_secret},
    routes::addition::domain::{
        InitializeProcessRequest, ReceiveLastPeerShareRequest, ReceivePeerShareRequest,
    },
};

#[async_trait::async_trait]
pub trait AdditionRepository: Send + Sync {
    async fn get_process(&self, process_id: Uuid) -> Result<AdditionProcess, anyhow::Error>;

    async fn create_process(
        &self,
        request: CreateProcessRequest,
    ) -> Result<AdditionProcess, anyhow::Error>;

    async fn receive_new_process_share(
        &self,
        request: InitializeProcessRequest,
    ) -> Result<AdditionProcess, anyhow::Error>;

    async fn receive_share(
        &self,
        request: ReceivePeerShareRequest,
    ) -> Result<AdditionProcess, anyhow::Error>;

    async fn receive_last_share(
        &self,
        request: ReceiveLastPeerShareRequest,
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
pub struct ReceivedShare {
    pub peer_id: u8,
    pub value: u64,
}

#[async_trait::async_trait]
impl AdditionRepository for InMemoryAdditionRepository {
    async fn create_process(
        &self,
        request: CreateProcessRequest,
    ) -> Result<AdditionProcess, anyhow::Error> {
        let process = AdditionProcess {
            id: request.process_id,
            input: request.input,
            own_share: request.own_share,
            shares_to_send: request.shares_to_send,
            received_shares: HashMap::new(),
            received_shares_sums: HashMap::new(),
            state: AdditionProcessState::AwaitingPeerShares,
        };
        let mut processes = self.processes.write().map_err(|e| {
            anyhow!("{e}").context("failed to acquire write lock on process creation")
        })?;
        processes.insert(request.process_id, process.clone());
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
        request: InitializeProcessRequest,
    ) -> Result<AdditionProcess, anyhow::Error> {
        let mut processes = self.processes.write().map_err(|e| {
            anyhow!("{e}").context("failed to acquire write lock on receiving new process share")
        })?;

        let mut received_shares = HashMap::new();
        received_shares.insert(request.from_peer_id, request.received_value);
        let process = AdditionProcess {
            id: request.process_id,
            input: request.input,
            own_share: request.own_share,
            shares_to_send: request.shares_to_send,
            received_shares,
            received_shares_sums: HashMap::new(),
            state: AdditionProcessState::AwaitingPeerShares,
        };
        processes.insert(request.process_id, process.clone());

        Ok(process)
    }

    async fn receive_share(
        &self,
        request: ReceivePeerShareRequest,
    ) -> Result<AdditionProcess, anyhow::Error> {
        let mut processes = self.processes.write().map_err(|e| {
            anyhow!("{e}").context("failed to acquire write lock on receiving share")
        })?;
        let process = processes
            .get_mut(&request.process_id)
            .ok_or(anyhow!("process not found"))?;
        if process.received_shares.contains_key(&request.from_peer_id) {
            return Err(anyhow!(
                "share from peer {} already received",
                request.from_peer_id
            ));
        }
        process
            .received_shares
            .insert(request.from_peer_id, request.received_value);

        Ok(process.clone())
    }

    async fn receive_last_share(
        &self,
        request: ReceiveLastPeerShareRequest,
    ) -> Result<AdditionProcess, anyhow::Error> {
        let mut processes = self.processes.write().map_err(|e| {
            anyhow!("{e}").context("failed to acquire write lock on receiving last share")
        })?;
        let process = processes
            .get_mut(&request.process_id)
            .ok_or_else(|| anyhow!("process not found"))?;
        if process.received_shares.contains_key(&request.from_peer_id) {
            return Err(anyhow!(
                "share from peer {} already received",
                request.from_peer_id
            ));
        }
        if !matches!(process.state, AdditionProcessState::AwaitingPeerShares) {
            return Err(anyhow!("process not in a state to receive last share"));
        }
        process
            .received_shares
            .insert(request.from_peer_id, request.received_value);

        process.state = AdditionProcessState::AwaitingPeerSharesSum {
            shares_sum: request.computed_shares_sum,
        };

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
        let mut input_shares = split_secret(input, &self.all_ids(), PRIME);
        let own_share = input_shares
            .remove(&self.server_peer_id)
            .ok_or(anyhow!("server peer share not found"))?;

        let mut received_shares_sums = HashMap::new();
        received_shares_sums.insert(from_peer_id, value);
        let process = AdditionProcess {
            id: process_id,
            input,
            own_share,
            shares_to_send: input_shares,
            received_shares: HashMap::new(),
            received_shares_sums,
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
        process.received_shares_sums.insert(from_peer_id, value);

        if let AdditionProcessState::AwaitingPeerSharesSum { shares_sum } = &process.state
            && process.received_shares_sums.len() == self.peer_ids.len()
        {
            let mut all_shares_sums = vec![Share {
                point: self.server_peer_id,
                value: *shares_sum,
            }];
            for (i, v) in process.received_shares_sums.iter() {
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
