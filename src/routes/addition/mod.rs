use ::futures::{StreamExt, stream};
use anyhow::anyhow;
use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use tracing::{error, info};
use uuid::Uuid;

use crate::Peer;

use super::{ApiError, RouterState};

pub mod repository;
use repository::{AdditionProcessState, Share};

pub fn addition_router(server_peer_id: u8) -> Router<RouterState> {
    Router::new()
        .route("/", post(create_process).layer(Extension(server_peer_id)))
        .route(
            "/{id}/send-share",
            post(send_share).layer(Extension(server_peer_id)),
        )
        .route(
            "/{id}/receive-share",
            post(receive_share).layer(Extension(server_peer_id)),
        )
        .route(
            "/{id}/send-sum-share",
            post(send_sum_share).layer(Extension(server_peer_id)),
        )
        .route("/{id}/receive-sum-share", post(receive_sum_share))
        .route("/{id}", delete(delete_process))
        .route("/{id}", get(get_process))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CreatedProcessResponse {
    pub process_id: Uuid,
    pub input: u64,
}
#[axum::debug_handler]
async fn create_process(
    State(state): State<RouterState>,
    Extension(server_peer_id): Extension<u8>,
) -> Result<(StatusCode, Json<CreatedProcessResponse>), ApiError> {
    let process_id = Uuid::new_v4();

    let created_process = state
        .addition
        .create_process(process_id)
        .await
        .map_err(|e| e.context("creating addition process"))?;

    info!("addition process created");

    if let Err(e) = send_shares_to_peers(
        &state.peers,
        server_peer_id,
        process_id,
        "receive-share",
        &created_process.own_input_shares,
    )
    .await
    {
        error!("error sending initial shares to peers: {}", e);
    }

    Ok((
        StatusCode::OK,
        Json(CreatedProcessResponse {
            process_id,
            input: created_process.input,
        }),
    ))
}

async fn send_share(
    State(state): State<RouterState>,
    Path(id): Path<Uuid>,
    Extension(server_peer_id): Extension<u8>,
) -> Result<StatusCode, ApiError> {
    let process = state
        .addition
        .get_process(id)
        .await
        .map_err(|e| e.context("retrieving process before sending share"))?;

    if let Err(e) = send_shares_to_peers(
        &state.peers,
        server_peer_id,
        id,
        "receive-share",
        &process.own_input_shares,
    )
    .await
    {
        error!("error sending shares to peers: {}", e);
    }

    Ok(StatusCode::OK)
}

async fn receive_share(
    State(state): State<RouterState>,
    Path(process_id): Path<Uuid>,
    peer: Peer,
    Extension(server_peer_id): Extension<u8>,
    Json(payload): Json<SharePayload>,
) -> Result<StatusCode, ApiError> {
    println!("Received share from peer id {}", peer.id);

    if let Ok(existing_process) = state.addition.get_process(process_id).await {
        if !matches!(
            existing_process.state,
            AdditionProcessState::AwaitingPeerShares
        ) {
            return Err(ApiError::BadRequest(
                "process is not in a state to receive shares".to_string(),
            ));
        }
        let updated_process = state
            .addition
            .receive_share(
                process_id,
                Share {
                    peer_id: peer.id,
                    share: payload.share,
                },
            )
            .await
            .map_err(|e| e.context("receiving share for existing process"))?;

        if let AdditionProcessState::AwaitingSumShares(process_state) = &updated_process.state {
            let sum_shares = state
                .peers
                .iter()
                .map(|peer| Share {
                    peer_id: peer.id,
                    share: process_state.own_sum_share,
                })
                .collect::<Vec<Share>>();

            if let Err(e) = send_shares_to_peers(
                &state.peers,
                server_peer_id,
                process_id,
                "receive-sum-share",
                &sum_shares,
            )
            .await
            {
                error!("error sending sum shares to peers: {}", e);
            }
        }
    } else {
        let process = state
            .addition
            .receive_new_process_share(
                process_id,
                Share {
                    peer_id: peer.id,
                    share: payload.share,
                },
            )
            .await
            .map_err(|e| e.context("creating process after share reception"))?;

        if let Err(e) = send_shares_to_peers(
            &state.peers,
            server_peer_id,
            process_id,
            "receive-share",
            &process.own_input_shares,
        )
        .await
        {
            error!("error sending shares to peers: {}", e);
        }
    }

    Ok(StatusCode::OK)
}

async fn send_sum_share(
    State(state): State<RouterState>,
    Path(id): Path<Uuid>,
    Extension(server_peer_id): Extension<u8>,
) -> Result<StatusCode, ApiError> {
    let process = state
        .addition
        .get_process(id)
        .await
        .map_err(|e| e.context("retrieving process before sending sum share"))?;

    let sum_share = match &process.state {
        AdditionProcessState::AwaitingSumShares(process_state) => process_state.own_sum_share,
        _ => {
            return Err(ApiError::BadRequest(
                "process is not in a state to send sum shares".to_string(),
            ));
        }
    };

    let sum_shares = state
        .peers
        .iter()
        .map(|peer| Share {
            peer_id: peer.id,
            share: sum_share,
        })
        .collect::<Vec<Share>>();

    if let Err(e) = send_shares_to_peers(
        &state.peers,
        server_peer_id,
        id,
        "receive-sum-share",
        &sum_shares,
    )
    .await
    {
        error!("error sending sum shares to peers: {}", e);
    }

    Ok(StatusCode::OK)
}

async fn receive_sum_share(
    State(state): State<RouterState>,
    Path(process_id): Path<Uuid>,
    peer: Peer,
    Json(payload): Json<SharePayload>,
) -> Result<StatusCode, ApiError> {
    let existing_process = state
        .addition
        .get_process(process_id)
        .await
        .map_err(|e| e.context("retrieving process before receiving sum share"))?;

    if !matches!(
        existing_process.state,
        AdditionProcessState::AwaitingSumShares { .. }
    ) {
        return Err(ApiError::BadRequest(
            "process is not in a state to receive sum shares".to_string(),
        ));
    }

    let updated_process = state
        .addition
        .receive_sum_share(
            process_id,
            Share {
                peer_id: peer.id,
                share: payload.share,
            },
        )
        .await
        .map_err(|e| e.context("receiving sum share for existing process"))?;

    if let AdditionProcessState::Completed(final_state) = &updated_process.state {
        info!(
            "Addition process {} completed with final sum: {}",
            process_id, final_state.final_sum
        );
    }

    Ok(StatusCode::OK)
}

async fn delete_process(
    State(state): State<RouterState>,
    Path(process_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .addition
        .delete_process(process_id)
        .await
        .map_err(|e| e.context("deleting addition process"))?;

    Ok(StatusCode::OK)
}

#[derive(serde::Serialize, Deserialize)]
pub struct GetProcessResponse {
    pub process_id: Uuid,
    pub input: u64,
    pub sum: Option<u64>,
}

async fn get_process(
    State(state): State<RouterState>,
    Path(process_id): Path<Uuid>,
) -> Result<(StatusCode, Json<GetProcessResponse>), ApiError> {
    let process = state
        .addition
        .get_process(process_id)
        .await
        .map_err(|e| e.context("retrieving process"))?;
    let sum = match &process.state {
        AdditionProcessState::Completed(completed_state) => Some(completed_state.final_sum),
        _ => None,
    };
    Ok((
        StatusCode::OK,
        Json(GetProcessResponse {
            process_id,
            input: process.input,
            sum,
        }),
    ))
}

#[derive(Deserialize, Serialize)]
struct SharePayload {
    share: u64,
}

async fn send_shares_to_peers(
    peers: &[Peer],
    server_peer_id: u8,
    process_id: Uuid,
    path: &str,
    shares: &[Share],
) -> Result<(), anyhow::Error> {
    let client = reqwest::Client::new();

    let mut peer_payloads = Vec::with_capacity(peers.len());
    for peer in peers {
        let url = format!("{}/additions/{process_id}/{path}", peer.url);
        let share = shares
            .iter()
            .find(|s| s.peer_id == peer.id)
            .ok_or(anyhow!("share for peer id {} not found", peer.id))?
            .clone();
        let payload = SharePayload { share: share.share };
        peer_payloads.push((url, payload));
    }

    let bodies = stream::iter(peer_payloads)
        .map(|(url, payload)| {
            let client = &client;
            async move {
                let res = client
                    .post(&url)
                    .header("X-PEER-ID", server_peer_id.to_string())
                    .json(&payload)
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

    Ok(())
}
