use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    body::Body,
    extract::{MatchedPath, Request},
    http::Response,
};
use mpc_exploration::{
    Config, Peer,
    domains::additions::{
        orchestrator::setup_addition_process_orchestrator,
        repository::InMemoryAdditionProcessRepository,
    },
    peer_communication::setup_peer_communication,
    routes::app_router,
};
use tower_http::trace::TraceLayer;
use tracing::{Level, Span, error, info, info_span, level_filters::LevelFilter};
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

    let addition_process_repository = Arc::new(InMemoryAdditionProcessRepository::new());

    let (
        peer_client,
        peer_messages_sender,
        mut peer_messages_relayer,
        peer_messages_relayer_pinger,
    ) = setup_peer_communication(config.server_peer_id, &config.peers);
    tokio::spawn(async move {
        peer_messages_relayer.run().await;
    });
    tokio::spawn(async move {
        if let Err(e) = peer_messages_relayer_pinger.run().await {
            error!(
                "Peer messages relayer interval pinger encountered an error: {}",
                e
            );
        }
    });

    let (mut addition_process_orchestrator, addition_process_notifier) =
        setup_addition_process_orchestrator(
            addition_process_repository.clone(),
            peer_client,
            config.server_peer_id,
            &config.peers,
        );
    let addition_process_notifier = Arc::new(addition_process_notifier);
    tokio::spawn(async move {
        addition_process_orchestrator.run().await;
    });
    tokio::spawn({
        let addition_process_notifier = addition_process_notifier.clone();
        async move {
            addition_process_notifier
                .run_interval_ping(tokio::time::Duration::from_secs(1))
                .await;
        }
    });

    let app = app_router(
        &config,
        addition_process_repository,
        Arc::new(peer_messages_sender),
        addition_process_notifier,
    )
    .layer(
        TraceLayer::new_for_http()
            .make_span_with(|request: &Request<_>| {
                let matched_path = request
                    .extensions()
                    .get::<MatchedPath>()
                    .map(MatchedPath::as_str);

                info_span!(
                    "http_request",
                    method = ?request.method(),
                    matched_path,
                )
            })
            .on_response(
                |response: &Response<Body>, latency: Duration, _span: &Span| {
                    if response.status().is_server_error() {
                        error!("response: {} {latency:?}", response.status())
                    } else {
                        info!("response: {} {latency:?}", response.status())
                    }
                },
            ),
    );

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
