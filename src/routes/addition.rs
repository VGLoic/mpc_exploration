use ::futures::{StreamExt, stream};
use anyhow::anyhow;
use axum::{
    Extension, Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use serde::Deserialize;
use tracing::{error, info};

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
    completed_sums: Vec<u32>,
}

impl Default for AdditionState {
    fn default() -> Self {
        AdditionState {
            secret: rand::random::<u16>().into(),
            step: AdditionStep::WaitingForShares(ShareTracker {
                own_share: rand::random::<u16>().into(),
                own_share_sent: false,
                received_shares: vec![],
            }),
            completed_sums: vec![],
        }
    }
}

pub fn addition_router(server_peer_id: u8) -> Router<RouterState> {
    Router::new()
        .route(
            "/send-share",
            post(send_share).layer(Extension(server_peer_id)),
        )
        .route("/receive-share", post(receive_share))
        .route(
            "/send-sum-share",
            post(send_sum_share).layer(Extension(server_peer_id)),
        )
        .route("/receive-sum-share", post(receive_sum_share))
        .route("/reset", post(reset))
        .route("/last-sum", get(get_last_sum))
}

async fn send_share(
    State(state): State<RouterState>,
    Extension(server_peer_id): Extension<u8>,
) -> Result<StatusCode, ApiError> {
    let _share_to_send = match &state
        .addition
        .read()
        .map_err(|e| anyhow!("{e}").context("getting read lock on state"))?
        .step
    {
        AdditionStep::WaitingForShares(t) => t.own_share,
        _ => {
            return Err(ApiError::BadRequest(
                "the server is currently not sending shares".to_string(),
            ));
        }
    };

    let client = reqwest::Client::new();

    let urls = state
        .peers
        .iter()
        .map(|peer| format!("{}/addition/receive-share", peer.url))
        .collect::<Vec<String>>();
    let bodies = stream::iter(urls)
        .map(|url| {
            let client = &client;
            async move {
                let res = client
                    .post(&url)
                    .header("X-PEER_ID", server_peer_id.to_string())
                    .send()
                    .await
                    .map_err(|e| {
                        anyhow!("{e}").context(format!("sending share to peer URL: {}", url))
                    })?;
                if res.status().is_success() {
                    Ok(())
                } else {
                    Err(anyhow!("Failed to send share: {}", res.status()))
                }
            }
        })
        .buffer_unordered(2);

    bodies
        .for_each(|result: Result<(), anyhow::Error>| async {
            match result {
                Ok(_) => {}
                Err(e) => {
                    error!("Error sending share: {}", e);
                }
            }
        })
        .await;

    let mut w_state = state
        .addition
        .write()
        .map_err(|e| anyhow!("{e}").context("getting write lock on state"))?;
    let share_tracker = match &mut w_state.step {
        AdditionStep::WaitingForShares(t) => t,
        _ => {
            return Err(ApiError::BadRequest(
                "the server is currently not sending shares".to_string(),
            ));
        }
    };

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
        .map_err(|e| anyhow!("{e}").context("getting write lock on state"))?;
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
        share: rand::random::<u16>().into(), // Placeholder for received share
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
async fn send_sum_share(
    State(state): State<RouterState>,
    Extension(server_peer_id): Extension<u8>,
) -> Result<StatusCode, ApiError> {
    let _sum_share_to_send = match &state
        .addition
        .read()
        .map_err(|e| anyhow!("{e}").context("getting read lock on state"))?
        .step
    {
        AdditionStep::WaitingForSumShares(t) => t.own_share,
        _ => {
            return Err(ApiError::BadRequest(
                "the server is currently not sending sum shares".to_string(),
            ));
        }
    };

    let peer_urls = state
        .peers
        .iter()
        .map(|peer| format!("{}/addition/receive-sum-share", peer.url))
        .collect::<Vec<String>>();
    let client = reqwest::Client::new();
    let bodies = stream::iter(peer_urls)
        .map(|url| {
            let client = &client;
            async move {
                let res = client
                    .post(url)
                    .header("X-PEER_ID", server_peer_id.to_string())
                    .send()
                    .await
                    .map_err(|e| anyhow!("{e}"))?;
                if res.status().is_success() {
                    Ok(())
                } else {
                    Err(anyhow!("Failed to send sum share: {}", res.status()))
                }
            }
        })
        .buffer_unordered(2);
    bodies
        .for_each(|result: Result<(), anyhow::Error>| async {
            match result {
                Ok(_) => {}
                Err(e) => {
                    error!("Error sending sum share: {}", e);
                }
            }
        })
        .await;

    let mut w_state = state
        .addition
        .write()
        .map_err(|e| anyhow!("{e}").context("getting write lock on state"))?;
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
        w_state.secret = rand::random::<u16>().into();
        w_state.step = AdditionStep::WaitingForShares(ShareTracker {
            own_share: rand::random::<u16>().into(),
            own_share_sent: false,
            received_shares: vec![],
        });
        w_state.completed_sums.push(sum);
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
        .map_err(|e| anyhow!("{e}").context("getting write lock on state"))?;

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
        share: rand::random::<u16>().into(), // Placeholder for received sum share
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
        w_state.secret = rand::random::<u16>().into();
        w_state.step = AdditionStep::WaitingForShares(ShareTracker {
            own_share: rand::random::<u16>().into(),
            own_share_sent: false,
            received_shares: vec![],
        });
        w_state.completed_sums.push(sum);
    }

    info!("sum share received");

    // Implementation of receiving sum share
    Ok(StatusCode::OK)
}

async fn reset(State(state): State<RouterState>) -> Result<StatusCode, ApiError> {
    let mut w_state = state
        .addition
        .write()
        .map_err(|e| anyhow!("{e}").context("getting write lock on state"))?;
    w_state.secret = rand::random::<u16>().into();
    w_state.step = AdditionStep::WaitingForShares(ShareTracker {
        own_share: rand::random::<u16>().into(),
        own_share_sent: false,
        received_shares: vec![],
    });

    info!("addition state reset");

    Ok(StatusCode::OK)
}

#[derive(serde::Serialize, Deserialize)]
pub struct LastSumResponse {
    pub sum: u32,
}

async fn get_last_sum(
    State(state): State<RouterState>,
) -> Result<(StatusCode, Json<LastSumResponse>), ApiError> {
    let r_state = state
        .addition
        .read()
        .map_err(|e| anyhow!("{e}").context("getting read lock on state"))?;
    match r_state.completed_sums.last() {
        Some(sum) => Ok((StatusCode::OK, Json(LastSumResponse { sum: *sum }))),
        None => Err(ApiError::BadRequest(
            "No completed sums available".to_string(),
        )),
    }
}
