use axum::http::StatusCode;
use mpc_exploration::routes::GetHealthcheckResponse;

mod common;
use common::{default_test_config, setup_instance};

#[tokio::test]
async fn test_healthcheck() {
    let instance_state = setup_instance(default_test_config()).await.unwrap();

    let response = reqwest::get(format!("{}/health", &instance_state.server_url))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.json::<GetHealthcheckResponse>().await.unwrap().ok);
}
