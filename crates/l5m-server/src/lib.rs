#![forbid(unsafe_code)]
//! L5M HTTP server: exposes gated retrieval, real-time writes, health, and
//! Prometheus metrics over a small REST API. The principal (tenant/policy/trust)
//! is resolved from the request by a [`PrincipalProvider`] — never from the
//! request body — so the gates always run under an authenticated identity.

pub mod principal;
pub mod ratelimit;

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use l5m_core::{verify_audit_chain, AuditLog, MemoryStore, QueryRequest, RetrievalMode};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{Mutex, RwLock};
use tower_http::{limit::RequestBodyLimitLayer, trace::TraceLayer};

use principal::{AuthError, Principal, PrincipalProvider};

/// Maximum request body size (DoS guard); override with L5M_MAX_BODY_BYTES.
pub const DEFAULT_MAX_BODY_BYTES: usize = 1 << 20; // 1 MiB

/// Optional access-audit sink: a hash-chained log of every disclosure.
pub struct AuditSink {
    pub log: Mutex<AuditLog>,
    pub path: PathBuf,
}

pub struct AppState {
    pub store: RwLock<MemoryStore>,
    pub principal: Arc<dyn PrincipalProvider>,
    pub audit: Option<AuditSink>,
    /// Optional per-tenant rate limiter.
    pub rate_limiter: Option<RateLimiter>,
    /// Max request body size in bytes.
    pub max_body_bytes: usize,
}

impl AppState {
    /// Resolve + authorize the caller: principal, credential scope for the
    /// requested operation, then per-tenant rate limit.
    fn authorize(&self, headers: &HeaderMap, required: Scope) -> Result<Principal, ApiError> {
        let principal = self.principal.principal(headers)?;
        if principal.scope < required {
            return Err(ApiError::forbidden(format!(
                "credential scope {:?} does not permit this operation (requires {required:?})",
                principal.scope
            )));
        }
        if let Some(limiter) = &self.rate_limiter {
            if !limiter.allow(principal.tenant_id) {
                return Err(ApiError::too_many());
            }
        }
        Ok(principal)
    }
}

pub fn build_router(state: Arc<AppState>) -> Router {
    let max_body = state.max_body_bytes;
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/readyz", get(|| async { "ready" }))
        .route("/metrics", get(metrics))
        .route("/v1/query", post(query))
        .route("/v1/memories", post(insert))
        .route("/v1/memories/:id", axum::routing::delete(delete_memory))
        .route("/v1/audit/verify", get(audit_verify))
        .layer(RequestBodyLimitLayer::new(max_body))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn audit_verify(State(state): State<Arc<AppState>>) -> Response {
    match &state.audit {
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "audit log not enabled"})),
        )
            .into_response(),
        Some(audit) => match verify_audit_chain(&audit.path) {
            Ok(verified) => (
                StatusCode::OK,
                Json(json!({"verified": verified, "intact": true})),
            )
                .into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"intact": false, "error": e.to_string()})),
            )
                .into_response(),
        },
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

async fn metrics(State(state): State<Arc<AppState>>) -> Response {
    let body = state.store.read().await.metrics().render_prometheus();
    (
        StatusCode::OK,
        [("content-type", "text/plain; version=0.0.4")],
        body,
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
struct QueryBody {
    query: String,
    #[serde(default)]
    max_capsules: Option<usize>,
    #[serde(default)]
    as_of: Option<i64>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    embedding: Vec<f32>,
}

async fn query(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<QueryBody>,
) -> Result<Response, ApiError> {
    let principal = state.authorize(&headers, Scope::Read)?;
    let request = QueryRequest {
        query: body.query,
        tenant_id: principal.tenant_id,
        as_of: body.as_of.unwrap_or(i64::MAX),
        context_mask: principal.context_mask,
        policy_mask: principal.policy_mask,
        trust_floor: principal.trust_floor,
        max_capsules: body.max_capsules.unwrap_or(8),
        max_tokens: usize::MAX,
        include_supporting: false,
        include_contradictions: false,
        max_hops: 1,
        mode: parse_mode(body.mode.as_deref()),
        embedding: body.embedding,
    };
    let response = state
        .store
        .read()
        .await
        .query(&request)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Append a tamper-evident audit record of what was disclosed, if enabled.
    if let Some(audit) = &state.audit {
        if let Ok(probe) = request.to_probe() {
            let _ = audit
                .log
                .lock()
                .await
                .record(&probe, &response.frame, now_unix());
        }
    }
    Ok(Json(response).into_response())
}

async fn insert(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut capsule): Json<Value>,
) -> Result<Response, ApiError> {
    let principal = state.authorize(&headers, Scope::Write)?;
    // Enforce tenant ownership: a caller may only write into its own tenant,
    // regardless of what the body claims.
    capsule["tenant_id"] = json!(principal.tenant_id);
    state
        .store
        .write()
        .await
        .insert_json(&capsule)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok((StatusCode::CREATED, Json(json!({"status": "inserted"}))).into_response())
}

async fn delete_memory(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let _principal = state.authorize(&headers, Scope::Write)?;
    let id: u128 = id
        .parse()
        .map_err(|_| ApiError::bad_request("capsule id must be an integer".to_string()))?;
    state
        .store
        .write()
        .await
        .delete(id)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok((StatusCode::OK, Json(json!({"status": "deleted"}))).into_response())
}

fn parse_mode(mode: Option<&str>) -> RetrievalMode {
    match mode {
        Some("parent-aggregate") => RetrievalMode::ParentAggregate,
        Some("hybrid") => RetrievalMode::HybridBm25L5m,
        Some("rrf-parent") => RetrievalMode::RrfFusionParent,
        _ => RetrievalMode::L5m,
    }
}

/// API error → JSON response with an appropriate status code.
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn internal(message: String) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message,
        }
    }
    fn bad_request(message: String) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message,
        }
    }
    fn too_many() -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "rate limit exceeded".to_string(),
        }
    }
    fn forbidden(message: String) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message,
        }
    }
}

impl From<AuthError> for ApiError {
    fn from(err: AuthError) -> Self {
        let (status, message) = match err {
            AuthError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized".to_string()),
            AuthError::Missing(h) => (StatusCode::UNAUTHORIZED, format!("missing {h}")),
            AuthError::Invalid(h) => (StatusCode::BAD_REQUEST, format!("invalid {h}")),
        };
        Self { status, message }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

// Re-export so `main` and tests can construct a provider.
pub use principal::{
    HeaderPrincipalProvider, JwksPrincipalProvider, JwtPrincipalProvider, Scope, ScopedKey,
};
pub use ratelimit::RateLimiter;
pub type SharedState = Arc<AppState>;
