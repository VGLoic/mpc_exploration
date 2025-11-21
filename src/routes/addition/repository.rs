use std::{collections::HashMap, sync::RwLock};

use anyhow::anyhow;
use uuid::Uuid;

use super::domain::{AdditionProcess, AdditionProcessState, CreateProcessRequest};
use crate::{
    Peer,
    routes::addition::domain::{
        InitializeProcessRequest, ReceiveLastPeerShareRequest, ReceiveLastPeerSharesSumRequest,
        ReceivePeerShareRequest, ReceivePeerSharesSumRequest,
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
        request: InitializeProcessRequest,
    ) -> Result<AdditionProcess, anyhow::Error>;

    async fn receive_shares_sum(
        &self,
        request: ReceivePeerSharesSumRequest,
    ) -> Result<AdditionProcess, anyhow::Error>;

    async fn receive_last_shares_sum(
        &self,
        request: ReceiveLastPeerSharesSumRequest,
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
        request: InitializeProcessRequest,
    ) -> Result<AdditionProcess, anyhow::Error> {
        let mut processes = self.processes.write().map_err(|e| {
            anyhow!("{e}").context("failed to acquire write lock on receiving sum share")
        })?;

        let mut received_shares_sums = HashMap::new();
        received_shares_sums.insert(request.from_peer_id, request.received_value);
        let process = AdditionProcess {
            id: request.process_id,
            input: request.input,
            own_share: request.own_share,
            shares_to_send: request.shares_to_send,
            received_shares: HashMap::new(),
            received_shares_sums,
            state: AdditionProcessState::AwaitingPeerShares,
        };
        processes.insert(request.process_id, process.clone());

        Ok(process.clone())
    }

    async fn receive_shares_sum(
        &self,
        request: ReceivePeerSharesSumRequest,
    ) -> Result<AdditionProcess, anyhow::Error> {
        let mut processes = self.processes.write().map_err(|e| {
            anyhow!("{e}").context("failed to acquire write lock on receiving sum share")
        })?;
        let process = processes
            .get_mut(&request.process_id)
            .ok_or_else(|| anyhow!("process not found"))?;
        if process
            .received_shares_sums
            .contains_key(&request.from_peer_id)
        {
            return Err(anyhow!(
                "sum share from peer {} already received",
                request.from_peer_id
            ));
        }
        process
            .received_shares_sums
            .insert(request.from_peer_id, request.received_value);

        Ok(process.clone())
    }

    async fn receive_last_shares_sum(
        &self,
        request: ReceiveLastPeerSharesSumRequest,
    ) -> Result<AdditionProcess, anyhow::Error> {
        let mut processes = self.processes.write().map_err(|e| {
            anyhow!("{e}").context("failed to acquire write lock on receiving last sum share")
        })?;
        let process = processes
            .get_mut(&request.process_id)
            .ok_or_else(|| anyhow!("process not found"))?;
        if process
            .received_shares_sums
            .contains_key(&request.from_peer_id)
        {
            return Err(anyhow!(
                "sum share from peer {} already received",
                request.from_peer_id
            ));
        }
        if !matches!(
            process.state,
            AdditionProcessState::AwaitingPeerSharesSum { .. }
        ) {
            return Err(anyhow!("process not in a state to receive last sum share"));
        }
        process
            .received_shares_sums
            .insert(request.from_peer_id, request.received_value);

        process.state = AdditionProcessState::Completed {
            final_sum: request.final_sum,
        };

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
