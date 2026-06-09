#![forbid(unsafe_code)]
//! L5M server binary.
//!
//! Configuration via environment:
//!   L5M_BIND     bind address (default 0.0.0.0:8080)
//!   L5M_SEGMENTS comma-separated segment paths to load (optional; empty = start
//!                from a pure real-time store)
//!   L5M_API_KEY  if set, require a matching `X-L5M-Api-Key` header

use std::sync::Arc;

use l5m_core::MemoryStore;
use l5m_server::{build_router, AppState, HeaderPrincipalProvider};
use tokio::sync::RwLock;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=info".into()),
        )
        .init();

    let bind = std::env::var("L5M_BIND").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let api_key = std::env::var("L5M_API_KEY").ok();

    let store = match std::env::var("L5M_SEGMENTS") {
        Ok(list) if !list.trim().is_empty() => {
            let paths: Vec<&str> = list.split(',').map(str::trim).collect();
            match MemoryStore::open_segments(paths) {
                Ok(store) => store,
                Err(err) => {
                    tracing::error!(%err, "failed to open segments");
                    std::process::exit(1);
                }
            }
        }
        _ => MemoryStore::empty(),
    };

    let state = Arc::new(AppState {
        store: RwLock::new(store),
        principal: Arc::new(HeaderPrincipalProvider { api_key }),
    });

    let app = build_router(state);
    let listener = match tokio::net::TcpListener::bind(&bind).await {
        Ok(listener) => listener,
        Err(err) => {
            tracing::error!(%err, %bind, "failed to bind");
            std::process::exit(1);
        }
    };
    tracing::info!(%bind, "l5m-server listening");
    if let Err(err) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        tracing::error!(%err, "server error");
        std::process::exit(1);
    }
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}
