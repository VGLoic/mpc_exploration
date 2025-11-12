use axum::{
    Json, Router,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::Config;

pub fn app_router(_: &Config) -> Router {
    Router::new()
        .route("/health", get(get_healthcheck))
        .fallback(not_found_handler)
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
enum ApiError {
    NotFound,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::NotFound => (StatusCode::NOT_FOUND, "Not found").into_response(),
        }
    }
}
