use std::collections::HashMap;

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

use crate::{
    Peer,
    mpc::{Share, recover_secret},
};

use super::{ApiError, RouterState};

pub mod repository;
use repository::AdditionProcessState;

const PRIME: u64 = 1_000_000_007;

pub fn addition_router(server_peer_id: u8) -> Router<RouterState> {
    Router::new()
        .route("/", post(create_process))
        .route("/{id}/send-share", post(send_share))
        .route("/{id}/receive-share", post(receive_share))
        .route("/{id}/send-sum-share", post(send_sum_share))
        .route("/{id}/receive-shares-sum", post(receive_shares_sum))
        .route("/{id}", delete(delete_process))
        .route("/{id}", get(get_process))
        .layer(Extension(server_peer_id))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CreatedProcessResponse {
    pub process_id: Uuid,
    pub input: u64,
}
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
        process_id,
        server_peer_id,
        &state.peers,
        created_process.input_shares,
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

    if let Err(e) =
        send_shares_to_peers(id, server_peer_id, &state.peers, process.input_shares).await
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
    Json(payload): Json<PeerPayload>,
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
            .receive_share(process_id, peer.id, payload.value)
            .await
            .map_err(|e| e.context("receiving share for existing process"))?;

        if let AdditionProcessState::AwaitingSumShares = &updated_process.state {
            let shares_sum = ((*updated_process
                .input_shares
                .get(&server_peer_id)
                .ok_or(anyhow!("unable to find own share"))?
                as u128
                + updated_process
                    .peer_shares
                    .values()
                    .map(|&v| v as u128)
                    .sum::<u128>())
                % PRIME as u128) as u64;
            if let Err(e) =
                send_shares_sum_to_peers(process_id, server_peer_id, &state.peers, shares_sum).await
            {
                error!("error sending sum shares to peers: {}", e);
            }
        }
    } else {
        let process = state
            .addition
            .receive_new_process_share(process_id, peer.id, payload.value)
            .await
            .map_err(|e| e.context("creating process after share reception"))?;

        if let Err(e) = send_shares_to_peers(
            process_id,
            server_peer_id,
            &state.peers,
            process.input_shares,
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

    let shares_sum = match &process.state {
        AdditionProcessState::AwaitingSumShares => {
            let own_share = *process
                .input_shares
                .get(&server_peer_id)
                .ok_or(anyhow!("unable to find its own share"))?;
            ((own_share as u128
                + process
                    .peer_shares
                    .values()
                    .map(|&v| v as u128)
                    .sum::<u128>())
                % PRIME as u128) as u64
        }
        _ => {
            return Err(ApiError::BadRequest(
                "process is not in a state to send sum shares".to_string(),
            ));
        }
    };

    if let Err(e) = send_shares_sum_to_peers(id, server_peer_id, &state.peers, shares_sum).await {
        error!("error sending sum shares to peers: {}", e);
    }

    Ok(StatusCode::OK)
}

async fn receive_shares_sum(
    State(state): State<RouterState>,
    Path(process_id): Path<Uuid>,
    peer: Peer,
    Extension(server_peer_id): Extension<u8>,
    Json(payload): Json<PeerPayload>,
) -> Result<StatusCode, ApiError> {
    let existing_process = state
        .addition
        .get_process(process_id)
        .await
        .map_err(|e| e.context("retrieving process before receiving sum share"))?;

    if !matches!(
        existing_process.state,
        AdditionProcessState::AwaitingSumShares
    ) {
        return Err(ApiError::BadRequest(
            "process is not in a state to receive sum shares".to_string(),
        ));
    }

    let updated_process = state
        .addition
        .receive_shares_sum(process_id, peer.id, payload.value)
        .await
        .map_err(|e| e.context("receiving sum share for existing process"))?;

    if let AdditionProcessState::Completed = &updated_process.state {
        let own_share = *updated_process
            .input_shares
            .get(&server_peer_id)
            .ok_or(anyhow!("unable to find its own share"))?;
        let own_shares_sum = ((own_share as u128
            + updated_process
                .peer_shares
                .values()
                .map(|&v| v as u128)
                .sum::<u128>())
            % PRIME as u128) as u64;
        let mut all_shares_sums = vec![Share {
            point: server_peer_id,
            value: own_shares_sum,
        }];
        for (i, v) in updated_process.peer_shares_sums.iter() {
            all_shares_sums.push(Share {
                point: *i,
                value: *v,
            });
        }
        let final_sum = recover_secret(&all_shares_sums, PRIME)?;
        info!(
            "Addition process {} completed with final sum: {}",
            process_id, final_sum
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
    Extension(server_peer_id): Extension<u8>,
    Path(process_id): Path<Uuid>,
) -> Result<(StatusCode, Json<GetProcessResponse>), ApiError> {
    let process = state
        .addition
        .get_process(process_id)
        .await
        .map_err(|e| e.context("retrieving process"))?;
    let sum = match &process.state {
        AdditionProcessState::Completed => {
            let own_share = *process
                .input_shares
                .get(&server_peer_id)
                .ok_or(anyhow!("unable to find its own share"))?;
            let own_shares_sum = ((own_share as u128
                + process
                    .peer_shares
                    .values()
                    .map(|&v| v as u128)
                    .sum::<u128>())
                % PRIME as u128) as u64;
            let mut all_shares_sums = vec![Share {
                point: server_peer_id,
                value: own_shares_sum,
            }];
            for (i, v) in process.peer_shares_sums.iter() {
                all_shares_sums.push(Share {
                    point: *i,
                    value: *v,
                });
            }
            let final_sum = recover_secret(&all_shares_sums, PRIME)?;
            Some(final_sum)
        }
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
struct PeerPayload {
    value: u64,
}
async fn send_shares_to_peers(
    process_id: Uuid,
    sender_peer_id: u8,
    peers: &[Peer],
    shares: HashMap<u8, u64>,
) -> Result<(), anyhow::Error> {
    let client = reqwest::Client::new();

    let mut peer_payloads = Vec::with_capacity(peers.len());
    for peer in peers {
        let url = format!("{}/additions/{process_id}/receive-share", peer.url);
        let value = *shares
            // The peer ID is equal to the point at which the polynomial is evaluated
            .get(&peer.id)
            .ok_or(anyhow!("share for peer id {} not found", peer.id))?;
        let payload = PeerPayload { value };
        peer_payloads.push((url, payload));
    }

    let bodies = stream::iter(peer_payloads)
        .map(|(url, payload)| {
            let client = &client;
            async move {
                let res = client
                    .post(&url)
                    .header("X-PEER-ID", sender_peer_id.to_string())
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
async fn send_shares_sum_to_peers(
    process_id: Uuid,
    sender_peer_id: u8,
    peers: &[Peer],
    shares_sum: u64,
) -> Result<(), anyhow::Error> {
    let client = reqwest::Client::new();

    let mut peer_payloads = Vec::with_capacity(peers.len());
    for peer in peers {
        let url = format!("{}/additions/{process_id}/receive-shares-sum", peer.url);
        let payload = PeerPayload { value: shares_sum };
        peer_payloads.push((url, payload));
    }

    let bodies = stream::iter(peer_payloads)
        .map(|(url, payload)| {
            let client = &client;
            async move {
                let res = client
                    .post(&url)
                    .header("X-PEER-ID", sender_peer_id.to_string())
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
