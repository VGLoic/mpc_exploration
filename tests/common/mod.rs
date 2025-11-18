use std::net::SocketAddr;

use mpc_exploration::{Config, Peer, routes::app_router};
use tower_http::trace::TraceLayer;
use tracing::{Level, info, level_filters::LevelFilter};
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

#[allow(dead_code)]
pub struct InstanceState {
    pub server_url: String,
}

#[allow(dead_code)]
pub fn default_test_config() -> Config {
    Config {
        port: 0,
        log_level: Level::WARN,
        server_peer_id: 1,
        peers: vec![
            Peer::new(2, "http://localhost:3001".to_string()),
            Peer::new(3, "http://localhost:3002".to_string()),
        ],
    }
}

pub async fn setup_instance(config: Config) -> Result<InstanceState, anyhow::Error> {
    let _ = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer().with_filter(LevelFilter::from_level(config.log_level)),
        )
        .try_init();

    let peer_communication = mpc_exploration::communication::HttpPeerCommunication::new(
        config.server_peer_id,
        &config.peers,
    );

    let app = app_router(&config, peer_communication).layer(TraceLayer::new_for_http());

    let listener = if config.port == 0 {
        bind_listener_to_free_port().await?
    } else {
        let addr = SocketAddr::from(([127, 0, 0, 1], config.port));
        tokio::net::TcpListener::bind(&addr).await.map_err(|err| {
            anyhow::anyhow!("Failed to bind the TCP listener to address {addr}: {err}")
        })?
    };

    let addr = listener.local_addr().unwrap();

    info!("Successfully bound the TCP listener to address {addr}\n");

    // Start a server, the handle is kept in order to abort it if needed
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    Ok(InstanceState {
        server_url: format!("http://{}:{}", addr.ip(), addr.port()),
    })
}

async fn bind_listener_to_free_port() -> Result<tokio::net::TcpListener, anyhow::Error> {
    for port in 51_000..60_000 {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(listener) => return Ok(listener),
            Err(_) => continue,
        }
    }
    Err(anyhow::anyhow!(
        "No free port found in the range 51000-60000"
    ))
}
