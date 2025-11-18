use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use tracing::{error, info};
use uuid::Uuid;

use crate::{
    Peer,
    communication::{PeerMessage, PeerMessagePayload},
};

use super::{ApiError, RouterState};

pub mod repository;
use repository::AdditionProcessState;

pub fn addition_router() -> Router<RouterState> {
    Router::new()
        .route("/", post(create_process))
        .route("/{id}/send-share", post(send_share))
        .route("/{id}/send-sum-share", post(send_sum_share))
        .route("/{id}", delete(delete_process))
        .route("/{id}", get(get_process))
        .route("/{id}/receive", post(receive_peer_message))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CreatedProcessResponse {
    pub process_id: Uuid,
    pub input: u64,
}
async fn create_process(
    State(state): State<RouterState>,
) -> Result<(StatusCode, Json<CreatedProcessResponse>), ApiError> {
    let process_id = Uuid::new_v4();

    let created_process = state
        .addition
        .create_process(process_id)
        .await
        .map_err(|e| e.context("creating addition process"))?;

    info!("addition process created");

    let peer_messages = created_process
        .input_shares
        .iter()
        .map(|(&peer_id, &value)| {
            (
                peer_id,
                crate::communication::PeerMessage::new_share_message(process_id, value),
            )
        })
        .filter(|(peer_id, _)| *peer_id != state.server_peer_id)
        .collect::<Vec<_>>();
    if let Err(e) = state.peer_communication.send_messages(peer_messages).await {
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
) -> Result<StatusCode, ApiError> {
    let process = state
        .addition
        .get_process(id)
        .await
        .map_err(|e| e.context("retrieving process before sending share"))?;

    let peer_messages = process
        .input_shares
        .iter()
        .map(|(&peer_id, &value)| {
            (
                peer_id,
                crate::communication::PeerMessage::new_share_message(id, value),
            )
        })
        .filter(|(peer_id, _)| *peer_id != state.server_peer_id)
        .collect::<Vec<_>>();
    if let Err(e) = state.peer_communication.send_messages(peer_messages).await {
        error!("error sending shares to peers: {}", e);
    }

    Ok(StatusCode::OK)
}

async fn send_sum_share(
    State(state): State<RouterState>,
    Path(process_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let process = state
        .addition
        .get_process(process_id)
        .await
        .map_err(|e| e.context("retrieving process before sending sum share"))?;

    let shares_sum = match &process.state {
        AdditionProcessState::AwaitingPeerSharesSum { shares_sum } => *shares_sum,
        _ => {
            return Err(ApiError::BadRequest(
                "process is not in a state to send sum shares".to_string(),
            ));
        }
    };

    let peer_messages = state
        .peers
        .iter()
        .map(|peer| {
            (
                peer.id,
                crate::communication::PeerMessage::new_shares_sum_message(process_id, shares_sum),
            )
        })
        .collect::<Vec<_>>();
    if let Err(e) = state.peer_communication.send_messages(peer_messages).await {
        error!("error sending sum shares to peers: {}", e);
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
        AdditionProcessState::Completed { final_sum } => Some(*final_sum),
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

async fn receive_peer_message(
    State(state): State<RouterState>,
    Path(process_id): Path<Uuid>,
    peer: Peer,
    Json(payload): Json<PeerMessagePayload>,
) -> Result<StatusCode, ApiError> {
    match payload {
        PeerMessagePayload::Share { value } => receive_share(state, process_id, peer, value).await,
        PeerMessagePayload::SharesSum { value } => {
            receive_shares_sum(state, process_id, peer, value).await
        }
    }
}

async fn receive_share(
    state: RouterState,
    process_id: Uuid,
    peer: Peer,
    value: u64,
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
            .receive_share(process_id, peer.id, value)
            .await
            .map_err(|e| e.context("receiving share for existing process"))?;

        if let AdditionProcessState::AwaitingPeerSharesSum { shares_sum } = &updated_process.state
            && let Err(e) = state
                .peer_communication
                .send_messages(
                    state
                        .peers
                        .iter()
                        .map(|peer| {
                            (
                                peer.id,
                                PeerMessage::new_shares_sum_message(process_id, *shares_sum),
                            )
                        })
                        .collect(),
                )
                .await
        {
            error!("error sending sum shares to peers: {}", e);
        }
    } else {
        let process = state
            .addition
            .receive_new_process_share(process_id, peer.id, value)
            .await
            .map_err(|e| e.context("creating process after share reception"))?;

        let peer_messages = process
            .input_shares
            .iter()
            .map(|(&peer_id, &value)| {
                (
                    peer_id,
                    crate::communication::PeerMessage::new_share_message(process_id, value),
                )
            })
            .filter(|(peer_id, _)| *peer_id != state.server_peer_id)
            .collect::<Vec<_>>();
        if let Err(e) = state.peer_communication.send_messages(peer_messages).await {
            error!("error sending shares to peers: {}", e);
        }
    }

    Ok(StatusCode::OK)
}

async fn receive_shares_sum(
    state: RouterState,
    process_id: Uuid,
    peer: Peer,
    value: u64,
) -> Result<StatusCode, ApiError> {
    if let Ok(existing_process) = state.addition.get_process(process_id).await {
        if !matches!(
            existing_process.state,
            AdditionProcessState::AwaitingPeerSharesSum { .. }
        ) {
            return Err(ApiError::BadRequest(
                "process is not in a state to receive sum shares".to_string(),
            ));
        }

        let updated_process = state
            .addition
            .receive_shares_sum(process_id, peer.id, value)
            .await
            .map_err(|e| e.context("receiving sum share for existing process"))?;

        if let AdditionProcessState::Completed { final_sum } = &updated_process.state {
            info!(
                "Addition process {} completed with final sum: {}",
                process_id, final_sum
            );
        }
    } else {
        let process = state
            .addition
            .receive_new_process_shares_sum(process_id, peer.id, value)
            .await
            .map_err(|e| e.context("creating process after sum share reception"))?;
        let peer_messages = process
            .input_shares
            .iter()
            .map(|(&peer_id, &value)| {
                (
                    peer_id,
                    crate::communication::PeerMessage::new_share_message(process_id, value),
                )
            })
            .filter(|(peer_id, _)| *peer_id != state.server_peer_id)
            .collect::<Vec<_>>();
        if let Err(e) = state.peer_communication.send_messages(peer_messages).await {
            error!("error sending shares to peers: {}", e);
        }
    }

    Ok(StatusCode::OK)
}
