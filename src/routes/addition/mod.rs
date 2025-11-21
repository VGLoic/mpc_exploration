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

pub mod domain;
pub mod repository;
use domain::AdditionProcessState;

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
    let create_process_request = domain::CreateProcessRequest::new(
        state.server_peer_id,
        &state.peers.iter().map(|p| p.id).collect::<Vec<_>>(),
    )
    .map_err(|e| match e {
        domain::CreateProcessRequestError::Unknown(err) => ApiError::from(err),
    })?;

    let created_process = state
        .addition
        .create_process(create_process_request)
        .await
        .map_err(|e| e.context("creating addition process"))?;

    info!("addition process {} created", created_process.id);

    let peer_messages = created_process
        .shares_to_send
        .iter()
        .map(|(&peer_id, &value)| {
            PeerMessage::new_share_message(peer_id, created_process.id, value)
        })
        .collect::<Vec<_>>();
    if let Err(e) = state.peer_communication.send_messages(peer_messages).await {
        error!("error sending initial shares to peers: {}", e);
    }

    Ok((
        StatusCode::OK,
        Json(CreatedProcessResponse {
            process_id: created_process.id,
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
        .shares_to_send
        .iter()
        .map(|(&peer_id, &value)| PeerMessage::new_share_message(peer_id, id, value))
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
        .map(|peer| PeerMessage::new_shares_sum_message(peer.id, process_id, shares_sum))
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

    let existing_process = state.addition.get_process(process_id).await.ok();
    let receive_share_request = domain::ReceiveShareRequest::new(
        process_id,
        state.server_peer_id,
        &state.peers.iter().map(|p| p.id).collect::<Vec<_>>(),
        peer.id,
        value,
        existing_process.as_ref(),
    )
    .map_err(|e| match e {
        domain::ReceiveShareRequestError::Unknown(err) => ApiError::from(err),
        domain::ReceiveShareRequestError::ShareAlreadyReceived(peer_id) => {
            ApiError::BadRequest(format!("share already received from peer id {peer_id}"))
        }
        domain::ReceiveShareRequestError::AllSharesReceived => ApiError::BadRequest(
            "all shares have already been received for this process".to_string(),
        ),
    })?;

    match receive_share_request {
        domain::ReceiveShareRequest::InitializeProcess(request) => {
            let created_process = state.addition.receive_new_process_share(request).await?;

            let peer_messages = created_process
                .shares_to_send
                .iter()
                .map(|(&peer_id, &value)| {
                    PeerMessage::new_share_message(peer_id, process_id, value)
                })
                .collect::<Vec<_>>();
            if let Err(e) = state.peer_communication.send_messages(peer_messages).await {
                error!("error sending shares to peers: {}", e);
            }
        }
        domain::ReceiveShareRequest::ReceiveShare(request) => {
            state.addition.receive_share(request).await?;
        }
        domain::ReceiveShareRequest::ReceiveLastShare(request) => {
            let sum_share_to_send = request.computed_shares_sum;

            state.addition.receive_last_share(request).await?;

            if let Err(e) = state
                .peer_communication
                .send_messages(
                    state
                        .peers
                        .iter()
                        .map(|peer| {
                            PeerMessage::new_shares_sum_message(
                                peer.id,
                                process_id,
                                sum_share_to_send,
                            )
                        })
                        .collect(),
                )
                .await
            {
                error!("error sending sum shares to peers: {}", e);
            }
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
    let existing_process = state.addition.get_process(process_id).await.ok();

    let receive_shares_sum_request = domain::ReceiveSharesSumRequest::new(
        process_id,
        state.server_peer_id,
        &state.peers.iter().map(|p| p.id).collect::<Vec<_>>(),
        peer.id,
        value,
        existing_process.as_ref(),
    )
    .map_err(|e| match e {
        domain::ReceiveSharesSumRequestError::Unknown(err) => ApiError::from(err),
        domain::ReceiveSharesSumRequestError::SumSharesAlreadyReceived(peer_id) => {
            ApiError::BadRequest(format!(
                "shares sum already received from peer id {peer_id}"
            ))
        }
        domain::ReceiveSharesSumRequestError::AllSharesSumsReceived => ApiError::BadRequest(
            "all shares sums have already been received for this process".to_string(),
        ),
    })?;

    match receive_shares_sum_request {
        domain::ReceiveSharesSumRequest::InitializeProcess(request) => {
            let created_process = state
                .addition
                .receive_new_process_shares_sum(request)
                .await?;

            let peer_messages = created_process
                .shares_to_send
                .iter()
                .map(|(&peer_id, &value)| {
                    PeerMessage::new_share_message(peer_id, process_id, value)
                })
                .collect::<Vec<_>>();
            if let Err(e) = state.peer_communication.send_messages(peer_messages).await {
                error!("error sending shares to peers: {}", e);
            }
        }
        domain::ReceiveSharesSumRequest::ReceiveSharesSum(request) => {
            state.addition.receive_shares_sum(request).await?;
        }
        domain::ReceiveSharesSumRequest::ReceiveLastSharesSum(request) => {
            let final_sum = request.final_sum;

            state.addition.receive_last_shares_sum(request).await?;

            info!(
                "Addition process {} completed with final sum: {}",
                process_id, final_sum
            );
        }
    }

    Ok(StatusCode::OK)
}
