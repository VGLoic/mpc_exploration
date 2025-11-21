use std::collections::HashMap;

use crate::mpc;
use thiserror::Error;
use uuid::Uuid;

const PRIME: u64 = 1_000_000_007;

#[derive(Clone)]
pub struct AdditionProcess {
    pub id: Uuid,
    pub input: u64,
    pub own_share: u64,
    pub shares_to_send: HashMap<u8, u64>,
    pub received_shares: HashMap<u8, u64>,
    pub received_shares_sums: HashMap<u8, u64>,
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
    pub own_share: u64,
    pub shares_to_send: HashMap<u8, u64>,
}

#[derive(Debug, Error)]
pub enum CreateProcessRequestError {
    #[error(transparent)]
    Unknown(#[from] anyhow::Error),
}

impl CreateProcessRequest {
    pub fn new(server_peer_id: u8, peer_ids: &[u8]) -> Result<Self, CreateProcessRequestError> {
        let process_id = uuid::Uuid::new_v4();
        let bootstrap = bootstrap_process(server_peer_id, peer_ids)?;
        Ok(Self {
            process_id,
            input: bootstrap.input,
            own_share: bootstrap.own_share,
            shares_to_send: bootstrap.shares_to_send,
        })
    }
}

// #######################################################
// ################### SHARE RECEPTION ###################
// #######################################################

pub enum ReceiveShareRequest {
    InitializeProcess(InitializeProcessRequest),
    ReceiveShare(ReceivePeerShareRequest),
    ReceiveLastShare(ReceiveLastPeerShareRequest),
}

pub struct InitializeProcessRequest {
    pub process_id: Uuid,
    pub input: u64,
    pub own_share: u64,
    pub shares_to_send: HashMap<u8, u64>,
    pub from_peer_id: u8,
    pub received_value: u64,
}

pub struct ReceivePeerShareRequest {
    pub process_id: Uuid,
    pub from_peer_id: u8,
    pub received_value: u64,
}

pub struct ReceiveLastPeerShareRequest {
    pub process_id: Uuid,
    pub from_peer_id: u8,
    pub received_value: u64,
    pub computed_shares_sum: u64,
}

#[derive(Debug, Error)]
pub enum ReceiveShareRequestError {
    #[error(transparent)]
    Unknown(#[from] anyhow::Error),
    #[error("share already received from peer id {0}")]
    ShareAlreadyReceived(u8),
    #[error("all shares have already been received for this process")]
    AllSharesReceived,
}

impl ReceiveShareRequest {
    pub fn new(
        process_id: Uuid,
        server_peer_id: u8,
        peer_ids: &[u8],
        from_peer_id: u8,
        received_value: u64,
        existing_process: Option<&AdditionProcess>,
    ) -> Result<Self, ReceiveShareRequestError> {
        if let Some(process) = existing_process {
            if !matches!(&process.state, AdditionProcessState::AwaitingPeerShares) {
                return Err(ReceiveShareRequestError::AllSharesReceived);
            }
            if process.received_shares.contains_key(&from_peer_id) {
                return Err(ReceiveShareRequestError::ShareAlreadyReceived(from_peer_id));
            }
            if process.received_shares.len() < peer_ids.len() - 1 {
                return Ok(ReceiveShareRequest::ReceiveShare(ReceivePeerShareRequest {
                    process_id,
                    from_peer_id,
                    received_value,
                }));
            }
            let computed_shares_sum = process
                .received_shares
                .values()
                .map(|v| Into::<u128>::into(*v))
                .sum::<u128>()
                .wrapping_add(process.own_share.into())
                .wrapping_add(received_value.into())
                .rem_euclid(PRIME.into()) as u64;

            // TODO: in theory, server could have received all the sum shares already but it is not handled here.
            // I think I will modify the process because it is complex to receive all possible actions from peers.

            return Ok(ReceiveShareRequest::ReceiveLastShare(
                ReceiveLastPeerShareRequest {
                    process_id,
                    from_peer_id,
                    received_value,
                    computed_shares_sum,
                },
            ));
        }

        let bootstrap = bootstrap_process(server_peer_id, peer_ids)?;
        Ok(ReceiveShareRequest::InitializeProcess(
            InitializeProcessRequest {
                process_id,
                input: bootstrap.input,
                own_share: bootstrap.own_share,
                shares_to_send: bootstrap.shares_to_send,
                from_peer_id,
                received_value,
            },
        ))
    }
}

// ###########################################################
// ################### SUM SHARE RECEPTION ###################
// ###########################################################

pub enum ReceiveSharesSumRequest {
    InitializeProcess(InitializeProcessRequest),
    ReceiveSharesSum(ReceivePeerSharesSumRequest),
    ReceiveLastSharesSum(ReceiveLastPeerSharesSumRequest),
}

pub struct ReceivePeerSharesSumRequest {
    pub process_id: Uuid,
    pub from_peer_id: u8,
    pub received_value: u64,
}

pub struct ReceiveLastPeerSharesSumRequest {
    pub process_id: Uuid,
    pub from_peer_id: u8,
    pub received_value: u64,
    pub final_sum: u64,
}

#[derive(Debug, Error)]
pub enum ReceiveSharesSumRequestError {
    #[error(transparent)]
    Unknown(#[from] anyhow::Error),
    #[error("shares sum already received from peer id {0}")]
    SumSharesAlreadyReceived(u8),
    #[error("all shares sums have already been received for this process")]
    AllSharesSumsReceived,
}

impl ReceiveSharesSumRequest {
    pub fn new(
        process_id: Uuid,
        server_peer_id: u8,
        peer_ids: &[u8],
        from_peer_id: u8,
        received_value: u64,
        existing_process: Option<&AdditionProcess>,
    ) -> Result<Self, ReceiveSharesSumRequestError> {
        if let Some(process) = existing_process {
            if process.received_shares_sums.contains_key(&from_peer_id) {
                return Err(ReceiveSharesSumRequestError::SumSharesAlreadyReceived(
                    from_peer_id,
                ));
            }

            match &process.state {
                AdditionProcessState::Completed { .. } => {
                    return Err(ReceiveSharesSumRequestError::AllSharesSumsReceived);
                }
                AdditionProcessState::AwaitingPeerShares => {
                    if process.received_shares_sums.contains_key(&from_peer_id) {
                        return Err(ReceiveSharesSumRequestError::SumSharesAlreadyReceived(
                            from_peer_id,
                        ));
                    }
                    return Ok(Self::ReceiveSharesSum(ReceivePeerSharesSumRequest {
                        process_id,
                        from_peer_id,
                        received_value,
                    }));
                }
                AdditionProcessState::AwaitingPeerSharesSum { shares_sum } => {
                    if process.received_shares_sums.contains_key(&from_peer_id) {
                        return Err(ReceiveSharesSumRequestError::SumSharesAlreadyReceived(
                            from_peer_id,
                        ));
                    }
                    if process.received_shares_sums.len() < peer_ids.len() - 1 {
                        return Ok(Self::ReceiveSharesSum(ReceivePeerSharesSumRequest {
                            process_id,
                            from_peer_id,
                            received_value,
                        }));
                    }
                    let mut all_shares_sums = vec![mpc::Share {
                        point: server_peer_id,
                        value: *shares_sum,
                    }];
                    for (i, v) in process.received_shares_sums.iter() {
                        all_shares_sums.push(mpc::Share {
                            point: *i,
                            value: *v,
                        });
                    }
                    all_shares_sums.push(mpc::Share {
                        point: from_peer_id,
                        value: received_value,
                    });
                    let final_sum = mpc::recover_secret(&all_shares_sums, PRIME)?;
                    return Ok(Self::ReceiveLastSharesSum(
                        ReceiveLastPeerSharesSumRequest {
                            process_id,
                            from_peer_id,
                            received_value,
                            final_sum,
                        },
                    ));
                }
            }
        }
        let bootstrap = bootstrap_process(server_peer_id, peer_ids)?;
        Ok(ReceiveSharesSumRequest::InitializeProcess(
            InitializeProcessRequest {
                process_id,
                input: bootstrap.input,
                own_share: bootstrap.own_share,
                shares_to_send: bootstrap.shares_to_send,
                from_peer_id,
                received_value,
            },
        ))
    }
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
