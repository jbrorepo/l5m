#![forbid(unsafe_code)]
//! L5M server binary.
//!
//! Configuration via environment:
//!   L5M_BIND     bind address (default 0.0.0.0:8080)
//!   L5M_SEGMENTS comma-separated segment paths to load (optional; empty = start
//!                from a pure real-time store)
//!   L5M_API_KEY  if set, require a matching `X-L5M-Api-Key` header
//!   L5M_API_KEYS scoped keys: comma-separated `secret:scope[:tenant]`
//!                (scope = read|write|admin; optional tenant binding)
//!   L5M_JWT_JWKS_FILE  path to a JWKS document for RS256 verification with
//!                kid-based key selection; hot-reloaded when the file changes
//!   L5M_DATA_DIR durable mode: WAL-backed writes that survive restarts; the
//!                newest checkpoint in <data>/checkpoints is auto-loaded as a
//!                base on startup. Without it the store is EPHEMERAL.
//!   L5M_CHECKPOINT_DIR directory for admin-triggered durable checkpoints
//!                (POST /v1/admin/checkpoint); defaults to
//!                $L5M_DATA_DIR/checkpoints when L5M_DATA_DIR is set
//!   L5M_DELTA_SEAL_THRESHOLD  active write-buffer size before sealing a run
//!   L5M_AUTO_COMPACT_RUNS     sealed-run count that triggers minor compaction
//!                             (0 disables automatic compaction)

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

    // Base segments named explicitly by the operator.
    let mut base_paths: Vec<std::path::PathBuf> = match std::env::var("L5M_SEGMENTS") {
        Ok(list) if !list.trim().is_empty() => list
            .split(',')
            .map(|s| std::path::PathBuf::from(s.trim()))
            .collect(),
        _ => Vec::new(),
    };

    // Durable mode: L5M_DATA_DIR enables a write-ahead log so acknowledged
    // writes survive restarts. Checkpoints default to <data>/checkpoints, and
    // the NEWEST checkpoint is auto-loaded as a base on startup — required for
    // correctness, because a checkpoint truncates the WAL (the data now lives
    // in the checkpoint segment, not the log).
    let data_dir = std::env::var("L5M_DATA_DIR")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(std::path::PathBuf::from);
    let checkpoint_dir = std::env::var("L5M_CHECKPOINT_DIR")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| data_dir.as_ref().map(|d| d.join("checkpoints")));
    if let Some(dir) = &checkpoint_dir {
        if let Err(err) = std::fs::create_dir_all(dir) {
            tracing::error!(%err, ?dir, "failed to create checkpoint dir");
            std::process::exit(1);
        }
        if let Some(newest) = newest_checkpoint(dir) {
            tracing::info!(?newest, "loading newest checkpoint as base");
            base_paths.push(newest);
        }
    }

    let store = match &data_dir {
        Some(dir) => {
            if let Err(err) = std::fs::create_dir_all(dir) {
                tracing::error!(%err, ?dir, "failed to create data dir");
                std::process::exit(1);
            }
            let wal = dir.join("l5m.wal");
            match MemoryStore::open_durable(&base_paths, &wal) {
                Ok(store) => {
                    tracing::info!(?wal, "durable mode: WAL-backed writes");
                    store
                }
                Err(err) => {
                    tracing::error!(%err, "failed to open durable store");
                    std::process::exit(1);
                }
            }
        }
        None if !base_paths.is_empty() => match MemoryStore::open_segments(&base_paths) {
            Ok(store) => store,
            Err(err) => {
                tracing::error!(%err, "failed to open segments");
                std::process::exit(1);
            }
        },
        None => {
            tracing::warn!("EPHEMERAL store: set L5M_DATA_DIR for durable writes");
            MemoryStore::empty()
        }
    };
    // Tune the real-time delta: L5M_DELTA_SEAL_THRESHOLD bounds the active write
    // buffer; L5M_AUTO_COMPACT_RUNS bounds query fan-out (set 0 to disable
    // automatic minor compaction).
    let store = match std::env::var("L5M_DELTA_SEAL_THRESHOLD")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        Some(n) => store.with_seal_threshold(n),
        None => store,
    };
    let store = match std::env::var("L5M_AUTO_COMPACT_RUNS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        Some(0) => store.with_auto_compaction(None),
        Some(n) => store.with_auto_compaction(Some(n)),
        None => store,
    };

    // Optional tamper-evident access audit log.
    let audit = match std::env::var("L5M_AUDIT_LOG") {
        Ok(path) if !path.trim().is_empty() => match l5m_core::AuditLog::open(&path) {
            Ok(log) => Some(l5m_server::AuditSink {
                log: tokio::sync::Mutex::new(log),
                path: path.into(),
            }),
            Err(err) => {
                tracing::error!(%err, "failed to open audit log");
                std::process::exit(1);
            }
        },
        _ => None,
    };

    // Principal resolution: prefer a verified JWT in production (JWKS file with
    // kid-based rotation, or a single HS256/RS256 key); fall back to the
    // header/API-key provider for dev / trusted networks.
    let principal: Arc<dyn l5m_server::principal::PrincipalProvider> =
        if let Ok(jwks_path) = std::env::var("L5M_JWT_JWKS_FILE") {
            match l5m_server::JwksPrincipalProvider::from_file(&jwks_path) {
                Ok(provider) => {
                    tracing::info!(%jwks_path, "auth: JWT RS256 via JWKS (hot-reload on change)");
                    Arc::new(provider)
                }
                Err(err) => {
                    tracing::error!(%err, "failed to load JWKS");
                    std::process::exit(1);
                }
            }
        } else if let Ok(secret) = std::env::var("L5M_JWT_HS256_SECRET") {
            tracing::info!("auth: JWT HS256");
            Arc::new(l5m_server::JwtPrincipalProvider::hs256(secret.as_bytes()))
        } else if let Ok(pem_path) = std::env::var("L5M_JWT_RS256_PEM_FILE") {
            match std::fs::read(&pem_path)
                .map_err(|e| e.to_string())
                .and_then(|pem| {
                    l5m_server::JwtPrincipalProvider::rs256_pem(&pem).map_err(|e| e.to_string())
                }) {
                Ok(provider) => {
                    tracing::info!("auth: JWT RS256");
                    Arc::new(provider)
                }
                Err(err) => {
                    tracing::error!(%err, "failed to load RS256 public key");
                    std::process::exit(1);
                }
            }
        } else if let Ok(list) = std::env::var("L5M_API_KEYS") {
            // Scoped keys: comma-separated `secret:scope[:tenant]` entries,
            // e.g. "k1:read,k2:write:7,k3:admin".
            let mut keys = Vec::new();
            for entry in list.split(',').filter(|e| !e.trim().is_empty()) {
                match l5m_server::ScopedKey::parse(entry) {
                    Ok(key) => keys.push(key),
                    Err(err) => {
                        tracing::error!(%err, "bad L5M_API_KEYS entry");
                        std::process::exit(1);
                    }
                }
            }
            tracing::info!(count = keys.len(), "auth: scoped API keys");
            Arc::new(HeaderPrincipalProvider::with_keys(keys))
        } else {
            tracing::warn!("auth: header/API-key provider (use JWT in production)");
            Arc::new(HeaderPrincipalProvider::from_optional_key(api_key))
        };

    // Optional per-tenant rate limiting + request body cap.
    let rate_limiter = std::env::var("L5M_RATE_PER_SEC")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .map(|rate| {
            let burst = std::env::var("L5M_RATE_BURST")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(rate.max(1.0));
            l5m_server::RateLimiter::new(rate, burst)
        });
    let max_body_bytes = std::env::var("L5M_MAX_BODY_BYTES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(l5m_server::DEFAULT_MAX_BODY_BYTES);

    let state = Arc::new(AppState {
        store: RwLock::new(store),
        principal,
        audit,
        rate_limiter,
        max_body_bytes,
        checkpoint_dir,
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

/// Newest `*.segment` file in the checkpoint directory (by modification time).
fn newest_checkpoint(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut newest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("segment") {
            continue;
        }
        let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if newest.as_ref().is_none_or(|(t, _)| modified > *t) {
            newest = Some((modified, path));
        }
    }
    newest.map(|(_, path)| path)
}
