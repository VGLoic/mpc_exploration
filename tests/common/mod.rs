use std::net::SocketAddr;

use mpc_exploration::{Config, routes::app_router};
use tower_http::trace::TraceLayer;
use tracing::{Level, info, level_filters::LevelFilter};
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

#[allow(dead_code)]
pub struct TestState {
    pub server_url: String,
}

pub async fn setup() -> Result<TestState, anyhow::Error> {
    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(LevelFilter::TRACE))
        .try_init();

    let config = Config {
        port: 0,
        log_level: Level::TRACE,
        peers: vec![],
    };

    let app = app_router(&config).layer(TraceLayer::new_for_http());

    // Giving 0 as port here will let the system dynamically find an available port
    // This is needed in order to let our test run in parallel
    let addr = SocketAddr::from(([127, 0, 0, 1], 0));
    let listener = tokio::net::TcpListener::bind(&addr).await.map_err(|err| {
        anyhow::anyhow!("Failed to bind the TCP listener to address {addr}: {err}")
    })?;

    let addr = listener.local_addr().unwrap();

    info!("Successfully bound the TCP listener to address {addr}\n");

    // Start a server, the handle is kept in order to abort it if needed
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    Ok(TestState {
        server_url: format!("http://{}:{}", addr.ip(), addr.port()),
    })
}
