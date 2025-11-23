use crate::mpc::{self, Share};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

pub mod orchestrator;
pub mod repository;

const PRIME: u64 = 1_000_000_007;

#[derive(Clone)]
pub struct AdditionProcess {
    pub id: Uuid,
    pub input: u64,
    pub own_share: u64,
    pub shares_to_send: HashMap<u8, u64>,
    // REMIND ME: reword state as we no longer need to track shares and shares sums simultaneously
    pub received_shares: HashMap<u8, u64>,
    pub received_shares_sums: HashMap<u8, u64>,
    pub state: AdditionProcessState,
}

#[derive(Clone)]
pub enum AdditionProcessState {
    AwaitingPeerShares,
    AwaitingPeerSharesSum { shares_sum: u64 },
    Completed { shares_sum: u64, final_sum: u64 },
}

// ########################################################
// ################### PROCESS CREATION ###################
// ########################################################

pub struct CreateProcessRequest {
    pub process_id: uuid::Uuid,
    pub input: u64,
    pub own_share: u64,
    pub shares_to_send: HashMap<u8, u64>,
}

#[derive(Debug, Error)]
pub enum CreateProcessRequestError {
    #[error(transparent)]
    Unknown(#[from] anyhow::Error),
}

impl CreateProcessRequest {
    pub fn new(
        process_id: uuid::Uuid,
        server_peer_id: u8,
        peer_ids: &[u8],
    ) -> Result<Self, CreateProcessRequestError> {
        let bootstrap = bootstrap_process(server_peer_id, peer_ids)?;
        Ok(Self {
            process_id,
            input: bootstrap.input,
            own_share: bootstrap.own_share,
            shares_to_send: bootstrap.shares_to_send,
        })
    }
}

// ########################################################
// ################### SHARES RECEPTION ###################
// ########################################################

#[derive(Debug)]
pub struct ReceiveSharesRequest {
    pub process_id: uuid::Uuid,
    /// New shares from peers
    pub received_shares: HashMap<u8, u64>,
    /// Computed shares sum if all shares have been registered
    pub computed_shares_sum: Option<u64>,
}

#[derive(Debug, Error)]
pub enum ReceiveSharesRequestError {
    #[error(transparent)]
    Unknown(#[from] anyhow::Error),
    #[error("process is not ready for shares reception")]
    InvalidState,
}

impl ReceiveSharesRequest {
    pub fn new(
        process: &AdditionProcess,
        received_shares: HashMap<u8, u64>,
        peers_count: usize,
    ) -> Result<Self, ReceiveSharesRequestError> {
        if !matches!(process.state, AdditionProcessState::AwaitingPeerShares) {
            return Err(ReceiveSharesRequestError::InvalidState);
        }
        let mut all_received_shares = process.received_shares.clone();
        for (peer_id, share) in &received_shares {
            all_received_shares.insert(*peer_id, *share);
        }
        if all_received_shares.len() < peers_count {
            return Ok(Self {
                process_id: process.id,
                received_shares: all_received_shares,
                computed_shares_sum: None,
            });
        }
        let computed_shares_sum = all_received_shares
            .values()
            .map(|v| Into::<u128>::into(*v))
            .sum::<u128>()
            .wrapping_add(process.own_share.into())
            .rem_euclid(PRIME as u128) as u64;
        Ok(Self {
            process_id: process.id,
            received_shares: all_received_shares,
            computed_shares_sum: Some(computed_shares_sum),
        })
    }
}

// #########################################################
// ################### SHARES SUMS RECEPTION ###############
// #########################################################

pub struct ReceiveSharesSumsRequest {
    pub process_id: uuid::Uuid,
    /// New shares sums from peers
    pub received_shares_sums: HashMap<u8, u64>,
    /// Computed final sum if all shares sums have been registered
    pub final_sum: Option<u64>,
}

#[derive(Debug, Error)]
pub enum ReceiveSharesSumsRequestError {
    #[error(transparent)]
    Unknown(#[from] anyhow::Error),
    #[error("process is not ready for shares sums reception")]
    InvalidState,
}

impl ReceiveSharesSumsRequest {
    pub fn new(
        process: &AdditionProcess,
        received_shares_sums: HashMap<u8, u64>,
        own_peer_id: u8,
        peers_count: usize,
    ) -> Result<Self, ReceiveSharesSumsRequestError> {
        if !matches!(
            process.state,
            AdditionProcessState::AwaitingPeerSharesSum { .. }
        ) {
            return Err(ReceiveSharesSumsRequestError::InvalidState);
        }
        let mut all_received_shares_sums = process.received_shares_sums.clone();
        for (peer_id, share_sum) in &received_shares_sums {
            all_received_shares_sums.insert(*peer_id, *share_sum);
        }
        if all_received_shares_sums.len() < peers_count {
            return Ok(Self {
                process_id: process.id,
                received_shares_sums: all_received_shares_sums,
                final_sum: None,
            });
        }
        let mut all_sums_coordinates = vec![Share {
            point: own_peer_id,
            value: process.own_share,
        }];
        for (peer_id, share_sum) in &all_received_shares_sums {
            all_sums_coordinates.push(Share {
                point: *peer_id,
                value: *share_sum,
            });
        }
        let final_sum = mpc::recover_secret(&all_sums_coordinates, PRIME)?;
        Ok(Self {
            process_id: process.id,
            received_shares_sums: all_received_shares_sums,
            final_sum: Some(final_sum),
        })
    }
}

// ############################################################
// ################### PROCESS PROGRESS #######################
// ############################################################
#[derive(Clone, Serialize, Deserialize)]
pub struct AdditionProcessProgress {
    pub share: u64,
    pub shares_sum: Option<u64>,
}

// ###########################################################
// ################### HELPER FUNCTIONS ######################
// ###########################################################

struct BootstrapProcessResult {
    pub input: u64,
    pub own_share: u64,
    pub shares_to_send: HashMap<u8, u64>,
}
fn bootstrap_process(
    server_peer_id: u8,
    peer_ids: &[u8],
) -> Result<BootstrapProcessResult, anyhow::Error> {
    let input = rand::random::<u16>().into();
    let all_ids = {
        let mut ids = peer_ids.to_vec();
        ids.push(server_peer_id);
        ids
    };
    let mut input_shares = mpc::split_secret(input, &all_ids, PRIME);
    let own_share = input_shares.remove(&server_peer_id).ok_or(anyhow::anyhow!(
        "own share missing for peer id {server_peer_id}"
    ))?;

    Ok(BootstrapProcessResult {
        input,
        own_share,
        shares_to_send: input_shares,
    })
}
