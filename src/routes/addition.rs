use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

use crate::{
    Peer, domains,
    peer_communication::{PeerMessage, peer_client::AdditionProcessProgress},
};

use super::{ApiError, RouterState};

pub fn addition_router() -> Router<RouterState> {
    Router::new()
        .route("/", post(create_process))
        .route("/{id}", delete(delete_process))
        .route("/{id}", get(get_process))
        .route("/{id}/progress", get(get_process_progress))
        .route(
            "/progress-notification",
            post(notify_internal_process_orchestrator),
        )
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CreatedProcessResponse {
    pub process_id: Uuid,
    pub input: u64,
}
#[derive(Serialize, Deserialize)]
pub struct CreateProcessHttpBody {
    pub process_id: Uuid,
}
async fn create_process(
    State(state): State<RouterState>,
    Json(payload): Json<CreateProcessHttpBody>,
) -> Result<(StatusCode, Json<CreatedProcessResponse>), ApiError> {
    let create_process_request = domains::additions::CreateProcessRequest::new(
        payload.process_id,
        state.server_peer_id,
        &state.peers.iter().map(|p| p.id).collect::<Vec<_>>(),
    )
    .map_err(|e| match e {
        domains::additions::CreateProcessRequestError::Unknown(err) => ApiError::from(err),
    })?;

    let created_process = state
        .addition
        .create_process(create_process_request)
        .await
        .map_err(|e| e.context("creating addition process"))?;

    info!("addition process {} created", created_process.id());

    if let Err(e) = state
        .peer_messages_sender
        .send_messages(
            state
                .peers
                .iter()
                .map(|p| PeerMessage::notify_process_progress(p.id))
                .collect(),
        )
        .await
    {
        tracing::error!("error sending initial shares to peers: {}", e);
    }

    Ok((
        StatusCode::OK,
        Json(CreatedProcessResponse {
            process_id: created_process.id(),
            input: created_process.input_shares().input,
        }),
    ))
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

    info!("addition process {process_id} deleted");

    Ok(StatusCode::OK)
}

#[derive(Serialize, Deserialize)]
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
    let sum = match &process {
        domains::additions::AdditionProcess::Completed(p) => Some(p.final_sum),
        _ => None,
    };
    Ok((
        StatusCode::OK,
        Json(GetProcessResponse {
            process_id,
            input: process.input_shares().input,
            sum,
        }),
    ))
}

async fn get_process_progress(
    State(state): State<RouterState>,
    peer: Peer,
    Path(process_id): Path<Uuid>,
) -> Result<Json<AdditionProcessProgress>, ApiError> {
    let process = state
        .addition
        .get_process(process_id)
        .await
        .map_err(|e| e.context("retrieving process before getting progress"))?;

    let peer_share = process
        .input_shares()
        .shares_to_send
        .get(&peer.id)
        .ok_or_else(|| ApiError::BadRequest("no share found for this peer".to_string()))?;
    let shares_sum = match &process {
        domains::additions::AdditionProcess::AwaitingPeerSharesSum(p) => Some(p.shares_sum),
        domains::additions::AdditionProcess::Completed(p) => Some(p.shares_sum),
        _ => None,
    };

    Ok(Json(AdditionProcessProgress {
        share: *peer_share,
        shares_sum,
    }))
}

async fn notify_internal_process_orchestrator(
    State(state): State<RouterState>,
    _peer: Peer,
) -> Result<StatusCode, ApiError> {
    state.addition_process_notifier.ping();

    Ok(StatusCode::OK)
}
