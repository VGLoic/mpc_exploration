use std::collections::HashMap;

use crate::mpc;
use thiserror::Error;
use uuid::Uuid;

const PRIME: u64 = 1_000_000_007;

#[derive(Clone)]
pub struct AdditionProcess {
    pub id: Uuid,
    pub input: u64,
    pub input_own_share: u64,
    pub input_peer_shares: HashMap<u8, u64>,
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

// ########################################################
// ################### PROCESS CREATION ###################
// ########################################################

pub struct CreateProcessRequest {
    pub process_id: uuid::Uuid,
    pub input: u64,
    pub input_own_share: u64,
    pub input_peer_shares: HashMap<u8, u64>,
}

#[derive(Debug, Error)]
pub enum CreateProcessRequestError {
    #[error("own share missing for peer id {0}")]
    OwnShareMissing(u8),
}

impl CreateProcessRequest {
    pub fn new(server_peer_id: u8, peer_ids: &[u8]) -> Result<Self, CreateProcessRequestError> {
        let process_id = uuid::Uuid::new_v4();
        let input = rand::random::<u16>().into();
        let all_ids = {
            let mut ids = peer_ids.to_vec();
            ids.push(server_peer_id);
            ids
        };
        let mut input_shares = mpc::split_secret(input, &all_ids, PRIME);
        let input_own_share = input_shares
            .remove(&server_peer_id)
            .ok_or(CreateProcessRequestError::OwnShareMissing(server_peer_id))?;
        Ok(Self {
            process_id,
            input,
            input_own_share,
            input_peer_shares: input_shares,
        })
    }
}
