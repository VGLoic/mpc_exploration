use anyhow::anyhow;
use axum::{Router, extract::State, http::StatusCode, routing::post};
use tracing::info;

use crate::Peer;

use super::{ApiError, RouterState};

#[derive(Clone, Debug)]
enum AdditionStep {
    WaitingForShares(ShareTracker),
    WaitingForSumShares(ShareTracker),
}

#[derive(Clone, Debug)]
struct ShareTracker {
    own_share: u32,
    own_share_sent: bool,
    received_shares: Vec<ReceivedShare>,
}

#[derive(Clone, Debug)]
struct ReceivedShare {
    peer: Peer,
    share: u32,
}

#[derive(Clone, Debug)]
pub struct AdditionState {
    secret: u32,
    step: AdditionStep,
}

impl AdditionState {
    pub fn new() -> Self {
        AdditionState {
            secret: rand::random::<u32>(),
            step: AdditionStep::WaitingForShares(ShareTracker {
                own_share: rand::random::<u32>(),
                own_share_sent: false,
                received_shares: vec![],
            }),
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
    let share_tracker = match &mut w_state.step {
        AdditionStep::WaitingForShares(t) => t,
        _ => {
            return Err(ApiError::BadRequest(
                "the server is currently not sending shares".to_string(),
            ));
        }
    };
    // Implementation of sending share
    share_tracker.own_share_sent = true;

    if share_tracker.own_share_sent && share_tracker.received_shares.len() == state.peers.len() {
        info!("All shares sent and received, moving to sum shares phase");
        let sum_share = share_tracker.own_share
            + share_tracker
                .received_shares
                .iter()
                .map(|s| s.share)
                .sum::<u32>();
        w_state.step = AdditionStep::WaitingForSumShares(ShareTracker {
            own_share: sum_share,
            own_share_sent: false,
            received_shares: vec![],
        });
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
    let share_tracker = match &mut w_state.step {
        AdditionStep::WaitingForShares(t) => t,
        _ => {
            return Err(ApiError::BadRequest(
                "the server is currently not sending shares".to_string(),
            ));
        }
    };

    if share_tracker
        .received_shares
        .iter()
        .any(|s| s.peer.id == peer.id)
    {
        info!("share already received");
        return Ok(StatusCode::OK);
    }

    share_tracker.received_shares.push(ReceivedShare {
        peer: peer.clone(),
        share: rand::random::<u32>(), // Placeholder for received share
    });

    if share_tracker.own_share_sent && share_tracker.received_shares.len() == state.peers.len() {
        info!("All shares sent and received, moving to sum shares phase");
        let sum_share = share_tracker.own_share
            + share_tracker
                .received_shares
                .iter()
                .map(|s| s.share)
                .sum::<u32>();
        w_state.step = AdditionStep::WaitingForSumShares(ShareTracker {
            own_share: sum_share,
            own_share_sent: false,
            received_shares: vec![],
        });
    }

    info!("share received");

    Ok(StatusCode::OK)
}
async fn send_sum_share(State(state): State<RouterState>) -> Result<StatusCode, ApiError> {
    let mut w_state = state
        .addition
        .write()
        .map_err(|e| ApiError::InternalServerError(anyhow!("{e}")))?;
    let share_tracker = match &mut w_state.step {
        AdditionStep::WaitingForSumShares(t) => t,
        _ => {
            return Err(ApiError::BadRequest(
                "the server is currently not sending sum shares".to_string(),
            ));
        }
    };
    // Implementation of sending sum share
    share_tracker.own_share_sent = true;

    if share_tracker.own_share_sent && share_tracker.received_shares.len() == state.peers.len() {
        let sum = share_tracker.own_share
            + share_tracker
                .received_shares
                .iter()
                .map(|s| s.share)
                .sum::<u32>();
        info!("final sum is: {}", sum);
        // Finalize addition process
        w_state.secret = rand::random::<u32>();
        w_state.step = AdditionStep::WaitingForShares(ShareTracker {
            own_share: rand::random::<u32>(),
            own_share_sent: false,
            received_shares: vec![],
        });
    }

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

    let share_tracker = match &mut w_state.step {
        AdditionStep::WaitingForSumShares(t) => t,
        _ => {
            return Err(ApiError::BadRequest(
                "the server is currently not ready to accept any sum shares".to_string(),
            ));
        }
    };
    if share_tracker
        .received_shares
        .iter()
        .any(|s| s.peer.id == peer.id)
    {
        info!("sum share already received");
        return Ok(StatusCode::OK);
    }

    share_tracker.received_shares.push(ReceivedShare {
        peer: peer.clone(),
        share: rand::random::<u32>(), // Placeholder for received sum share
    });

    if share_tracker.own_share_sent && share_tracker.received_shares.len() == state.peers.len() {
        let sum = share_tracker.own_share
            + share_tracker
                .received_shares
                .iter()
                .map(|s| s.share)
                .sum::<u32>();
        info!("final sum is: {}", sum);
        // Finalize addition process
        w_state.secret = rand::random::<u32>();
        w_state.step = AdditionStep::WaitingForShares(ShareTracker {
            own_share: rand::random::<u32>(),
            own_share_sent: false,
            received_shares: vec![],
        });
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
    w_state.step = AdditionStep::WaitingForShares(ShareTracker {
        own_share: rand::random::<u32>(),
        own_share_sent: false,
        received_shares: vec![],
    });

    info!("addition state reset");

    Ok(StatusCode::OK)
}
