use anyhow::anyhow;
use axum::{Router, extract::State, http::StatusCode, routing::post};
use tracing::info;

use crate::Peer;

use super::{ApiError, RouterState};

const OTHER_PARTIES: usize = 2;

#[derive(Clone, Debug)]
enum AdditionStatus {
    WaitingForShares {
        share: u32,
        own_share_sent: bool,
        other_shares: [Option<u32>; OTHER_PARTIES],
    },
    WaitingForSumShares {
        sum_share: u32,
        own_sum_share_sent: bool,
        other_sum_shares: [Option<u32>; OTHER_PARTIES],
    },
}

#[derive(Clone, Debug)]
pub struct AdditionState {
    secret: u32,
    status: AdditionStatus,
}

impl AdditionState {
    pub fn new() -> Self {
        AdditionState {
            secret: rand::random::<u32>(),
            status: AdditionStatus::WaitingForShares {
                share: rand::random::<u32>(),
                own_share_sent: false,
                other_shares: [None; OTHER_PARTIES],
            },
        }
    }
}

pub fn addition_router() -> Router<RouterState> {
    Router::new()
        .route("/send-share", post(send_share))
        .route("/receive-share", post(receive_share))
        .route("/send-sum-share", post(send_sum_share))
        .route("/receive-sum-share", post(receive_sum_share))
        .route("/reset", post(reset))
}

async fn send_share(State(state): State<RouterState>) -> Result<StatusCode, ApiError> {
    let mut w_state = state
        .addition
        .write()
        .map_err(|e| ApiError::InternalServerError(anyhow!("{e}")))?;
    let (own_share, other_shares, own_share_sent) = match &mut w_state.status {
        AdditionStatus::WaitingForShares {
            own_share_sent,
            share,
            other_shares,
        } => {
            *own_share_sent = true;
            (share, other_shares, own_share_sent)
        }
        _ => {
            return Err(ApiError::BadRequest(
                "the server is currently not sending shares".to_string(),
            ));
        }
    };

    if *own_share_sent && other_shares.iter().all(|s| s.is_some()) {
        info!("All shares sent and received, moving to sum shares phase");
        let sum_share = *own_share + other_shares.iter().map(|s| s.unwrap()).sum::<u32>();
        w_state.status = AdditionStatus::WaitingForSumShares {
            sum_share,
            own_sum_share_sent: false,
            other_sum_shares: [None; OTHER_PARTIES],
        };
    }

    info!("share sent");

    Ok(StatusCode::OK)
}
async fn receive_share(
    State(state): State<RouterState>,
    peer: Peer,
) -> Result<StatusCode, ApiError> {
    println!("Received share from {:?}", peer);
    let mut w_state = state
        .addition
        .write()
        .map_err(|e| ApiError::InternalServerError(anyhow!("{e}")))?;
    let (own_share, other_shares, own_share_sent) = match &mut w_state.status {
        AdditionStatus::WaitingForShares {
            other_shares,
            share,
            own_share_sent,
        } => (share, other_shares, own_share_sent),
        _ => {
            return Err(ApiError::BadRequest(
                "the server is currently not ready to accept any shares".to_string(),
            ));
        }
    };
    other_shares[0] = Some(rand::random::<u32>()); // Placeholder for received share
    other_shares[1] = Some(rand::random::<u32>()); // Placeholder for received share

    if *own_share_sent && other_shares.iter().all(|s| s.is_some()) {
        info!("All shares sent and received, moving to sum shares phase");
        let sum_share = *own_share + other_shares.iter().map(|s| s.unwrap()).sum::<u32>();
        w_state.status = AdditionStatus::WaitingForSumShares {
            sum_share,
            own_sum_share_sent: false,
            other_sum_shares: [None; OTHER_PARTIES],
        };
    }

    info!("share received");

    Ok(StatusCode::OK)
}
async fn send_sum_share(State(state): State<RouterState>) -> Result<StatusCode, ApiError> {
    let mut w_state = state
        .addition
        .write()
        .map_err(|e| ApiError::InternalServerError(anyhow!("{e}")))?;
    match &mut w_state.status {
        AdditionStatus::WaitingForSumShares {
            sum_share: _,
            own_sum_share_sent,
            ..
        } => {
            *own_sum_share_sent = true;
        }
        _ => {
            return Err(ApiError::BadRequest(
                "the server is currently not sending sum shares".to_string(),
            ));
        }
    };
    // Implementation of sending sum share
    info!("sum share sent");
    Ok(StatusCode::OK)
}

async fn receive_sum_share(
    State(state): State<RouterState>,
    peer: Peer,
) -> Result<StatusCode, ApiError> {
    println!("Received share from {:?}", peer);
    let mut w_state = state
        .addition
        .write()
        .map_err(|e| ApiError::InternalServerError(anyhow!("{e}")))?;
    let (own_sum_share, other_sum_shares, own_sum_share_sent) = match &mut w_state.status {
        AdditionStatus::WaitingForSumShares {
            other_sum_shares,
            own_sum_share_sent,
            sum_share,
        } => (sum_share, other_sum_shares, own_sum_share_sent),
        _ => {
            return Err(ApiError::BadRequest(
                "the server is currently not ready to accept any sum shares".to_string(),
            ));
        }
    };
    other_sum_shares[0] = Some(rand::random::<u32>()); // Placeholder for received sum share
    other_sum_shares[1] = Some(rand::random::<u32>()); // Placeholder for received sum share

    if *own_sum_share_sent && other_sum_shares.iter().all(|s| s.is_some()) {
        // Not correct
        let sum = *own_sum_share + other_sum_shares.iter().map(|s| s.unwrap()).sum::<u32>();
        info!("final sum is: {}", sum);
        // Finalize addition process
        w_state.secret = rand::random::<u32>();
        w_state.status = AdditionStatus::WaitingForShares {
            share: rand::random::<u32>(),
            own_share_sent: false,
            other_shares: [None; OTHER_PARTIES],
        };
    }

    info!("sum share received");

    // Implementation of receiving sum share
    Ok(StatusCode::OK)
}

async fn reset(State(state): State<RouterState>) -> Result<StatusCode, ApiError> {
    let mut w_state = state
        .addition
        .write()
        .map_err(|e| ApiError::InternalServerError(anyhow!("{e}")))?;
    w_state.secret = rand::random::<u32>();
    w_state.status = AdditionStatus::WaitingForShares {
        share: rand::random::<u32>(),
        own_share_sent: false,
        other_shares: [None; OTHER_PARTIES],
    };

    info!("addition state reset");

    Ok(StatusCode::OK)
}
