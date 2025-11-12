use axum::http::StatusCode;
mod common;

#[tokio::test]
async fn test_not_found() {
    let test_state = common::setup().await.unwrap();

    let response = reqwest::get(format!("{}/unknown-route", &test_state.server_url))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.text().await.unwrap(), "Not found");
}
