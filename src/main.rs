use std::{sync::Arc, time::Duration};

use axum::{
    body::Body,
    extract::{MatchedPath, Request},
    http::{HeaderName, Response},
};
use dotenvy::dotenv;
use mpc_exploration::{
    Config,
    domains::additions::{
        orchestrator::setup_addition_process_orchestrator,
        repository::InMemoryAdditionProcessRepository,
    },
    peer_communication::setup_peer_communication,
    routes::app_router,
};
use tokio::signal;
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing::{Span, error, info, info_span, level_filters::LevelFilter};
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

const REQUEST_ID_HEADER: &str = "x-request-id";

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    if let Err(err) = dotenv()
        && !err.not_found()
    {
        return Err(anyhow::anyhow!("Error while loading .env file: {err}"));
    }

    let config = match Config::parse_environment() {
        Ok(c) => c,
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to parse environment variables for configuration: {e}"
            ));
        }
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_filter(Into::<LevelFilter>::into(config.log_level)),
        )
        .init();

    let x_request_id = HeaderName::from_static(REQUEST_ID_HEADER);

    let addition_process_repository = Arc::new(InMemoryAdditionProcessRepository::new());

    let (mut addition_process_orchestrator, addition_process_orchestrator_pinger) =
        setup_addition_process_orchestrator(
            addition_process_repository.clone(),
            config.server_peer_id,
            &config.peers,
        );
    tokio::spawn(async move {
        addition_process_orchestrator.run().await;
    });
    tokio::spawn(async move {
        if let Err(e) = addition_process_orchestrator_pinger.run().await {
            error!(
                "Addition process interval pinger encountered an error: {}",
                e
            );
        }
    });

    let (peer_messages_sender, mut peer_messages_relayer, peer_messages_relayer_pinger) =
        setup_peer_communication(config.server_peer_id, &config.peers);
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

    let app = app_router(
        &config,
        addition_process_repository,
        Arc::new(peer_messages_sender),
    )
    .layer((
        // Set `x-request-id` header for every request
        SetRequestIdLayer::new(x_request_id.clone(), MakeRequestUuid),
        // Log request and response
        TraceLayer::new_for_http()
            .make_span_with(|request: &Request<_>| {
                let matched_path = request
                    .extensions()
                    .get::<MatchedPath>()
                    .map(MatchedPath::as_str);

                let request_id = request.headers().get(REQUEST_ID_HEADER);

                match request_id {
                    Some(v) => info_span!(
                        "http_request",
                        method = ?request.method(),
                        matched_path,
                        request_id = ?v
                    ),
                    None => {
                        error!("Failed to extract `request_id` header");
                        info_span!(
                            "http_request",
                            method = ?request.method(),
                            matched_path,
                        )
                    }
                }
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
        // Timeout requests at 10 seconds
        TimeoutLayer::new(Duration::from_secs(10)),
        // Propagate the `x-request-id` header to responses
        PropagateRequestIdLayer::new(x_request_id),
    ));

    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.map_err(|err| {
        let err = format!("Error while binding the TCP listener to address {addr}: {err}");

        error!(err);
        anyhow::anyhow!(err)
    })?;

    info!("Successfully bind the TCP listener to address {addr}\n");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|err| {
            let err = format!("Error while serving the routes: {err}");
            error!(err);
            anyhow::anyhow!(err)
        })?;

    info!("App has been gracefully shutdown");

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
