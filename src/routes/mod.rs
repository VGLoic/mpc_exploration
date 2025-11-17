use std::sync::Arc;

use axum::{
    Json, Router,
    extract::FromRequestParts,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use tracing::{error, warn};

use crate::{Config, Peer, routes::addition::repository::AdditionRepository};

pub mod addition;

#[derive(Clone)]
pub struct RouterState {
    addition: Arc<dyn AdditionRepository>,
    peers: Vec<Peer>,
}

pub fn app_router(config: &Config) -> Router {
    let state = RouterState {
        addition: Arc::new(addition::repository::InMemoryAdditionRepository::new(
            &config.peers,
        )),
        peers: config.peers.clone(),
    };
    Router::new()
        .route("/health", get(get_healthcheck))
        .nest(
            "/additions",
            addition::addition_router(config.server_peer_id),
        )
        .fallback(not_found_handler)
        .with_state(state)
}

#[derive(Serialize, Deserialize)]
pub struct GetHealthcheckResponse {
    pub ok: bool,
}
async fn get_healthcheck() -> (StatusCode, Json<GetHealthcheckResponse>) {
    (StatusCode::OK, Json(GetHealthcheckResponse { ok: true }))
}

async fn not_found_handler() -> impl IntoResponse {
    ApiError::NotFound
}

// ############################################
// ################## ERRORS ##################
// ############################################

#[derive(Debug)]
pub enum ApiError {
    NotFound,
    InternalServerError(anyhow::Error),
    BadRequest(String),
    Unauthorized(String),
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        ApiError::InternalServerError(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::NotFound => (StatusCode::NOT_FOUND, "Not found").into_response(),
            Self::InternalServerError(e) => {
                error!("Internal server error: {:?}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
            }
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
            Self::Unauthorized(msg) => {
                warn!("Unauthorized access attempt: {}", msg);
                StatusCode::UNAUTHORIZED.into_response()
            }
        }
    }
}

// ######################################################
// ################## PEER RESTRICTION ##################
// ######################################################

impl FromRequestParts<RouterState> for Peer {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &RouterState,
    ) -> Result<Self, Self::Rejection> {
        let peer_id = parts
            .headers
            .get("X-PEER-ID")
            .ok_or_else(|| ApiError::Unauthorized("Missing X-PEER-ID header".to_string()))?
            .to_str()
            .map_err(|e| ApiError::Unauthorized(format!("Invalid X-PEER-ID header: {e}")))?
            .parse::<u8>()
            .map_err(|e| ApiError::Unauthorized(format!("Invalid X-PEER-ID header: {e}")))?;
        let related_peer =
            state
                .peers
                .iter()
                .find(|peer| peer.id == peer_id)
                .ok_or(ApiError::Unauthorized(format!(
                    "Unauthorized peer: {}",
                    peer_id
                )))?;
        Ok(related_peer.clone())
    }
}
