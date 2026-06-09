// E4: server API integration tests — exercised in-process via tower oneshot (no
// real socket). Proves the auth hook, gated retrieval over HTTP, tenant-scoped
// writes, and the metrics endpoint.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use l5m_core::MemoryStore;
use l5m_server::{build_router, AppState, HeaderPrincipalProvider};
use tokio::sync::RwLock;
use tower::ServiceExt; // oneshot

fn app() -> axum::Router {
    app_full(None, None)
}

fn app_with_audit(audit: Option<l5m_server::AuditSink>) -> axum::Router {
    app_full(audit, None)
}

fn app_full(
    audit: Option<l5m_server::AuditSink>,
    rate_limiter: Option<l5m_server::RateLimiter>,
) -> axum::Router {
    let state = Arc::new(AppState {
        store: RwLock::new(MemoryStore::empty()),
        principal: Arc::new(HeaderPrincipalProvider::single("secret")),
        audit,
        rate_limiter,
        max_body_bytes: l5m_server::DEFAULT_MAX_BODY_BYTES,
    });
    build_router(state)
}

/// Router with an arbitrary principal provider (scoped-key / JWKS tests).
fn app_with_provider(provider: Arc<dyn l5m_server::principal::PrincipalProvider>) -> axum::Router {
    let state = Arc::new(AppState {
        store: RwLock::new(MemoryStore::empty()),
        principal: provider,
        audit: None,
        rate_limiter: None,
        max_body_bytes: l5m_server::DEFAULT_MAX_BODY_BYTES,
    });
    build_router(state)
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

fn insert_req(tenant: u64, capsule: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/memories")
        .header("x-l5m-api-key", "secret")
        .header("x-l5m-tenant", tenant.to_string())
        .header("content-type", "application/json")
        .body(Body::from(capsule.to_string()))
        .unwrap()
}

fn query_req(tenant: u64, q: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/query")
        .header("x-l5m-api-key", "secret")
        .header("x-l5m-tenant", tenant.to_string())
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({ "query": q, "as_of": 1000 }).to_string(),
        ))
        .unwrap()
}

#[tokio::test]
async fn healthz_and_auth() {
    let app = app();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Missing API key -> 401.
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/query")
                .header("x-l5m-tenant", "1")
                .header("content-type", "application/json")
                .body(Body::from("{\"query\":\"x\"}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn insert_then_query_over_http() {
    let app = app();
    let cap = serde_json::json!({
        "capsule_id":"1","tenant_id":1,
        "claim":"the violet passphrase is kelpstone","evidence":"the violet passphrase is kelpstone",
        "source_id":1,"valid_from":1,"observed_at":1,"last_verified_at":1,
        "context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,
        "classification":1,"poison_risk":0
    });
    let resp = app.clone().oneshot(insert_req(1, cap)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let resp = app
        .oneshot(query_req(1, "violet passphrase kelpstone"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    let claims = v["frame"]["capsules"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        claims
            .iter()
            .any(|c| c["claim"].as_str().unwrap_or("").contains("kelpstone")),
        "query should return the inserted memory"
    );
}

#[tokio::test]
async fn writes_are_tenant_scoped_and_isolated() {
    let app = app();
    // Tenant 1 writes a memory (even if the body lies about tenant, the server
    // forces the authenticated tenant).
    let cap = serde_json::json!({
        "capsule_id":"1","tenant_id":999,
        "claim":"tenant one secret kelpstone","evidence":"tenant one secret kelpstone",
        "source_id":1,"valid_from":1,"observed_at":1,"last_verified_at":1,
        "context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,
        "classification":1,"poison_risk":0
    });
    assert_eq!(
        app.clone()
            .oneshot(insert_req(1, cap))
            .await
            .unwrap()
            .status(),
        StatusCode::CREATED
    );

    // Tenant 2 must NOT see tenant 1's memory.
    let resp = app
        .clone()
        .oneshot(query_req(2, "secret kelpstone"))
        .await
        .unwrap();
    let v = body_json(resp).await;
    let caps = v["frame"]["capsules"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        caps.iter()
            .all(|c| !c["claim"].as_str().unwrap_or("").contains("kelpstone")),
        "tenant 2 must not see tenant 1 data"
    );

    // Tenant 1 does see it.
    let resp = app.oneshot(query_req(1, "secret kelpstone")).await.unwrap();
    let v = body_json(resp).await;
    let caps = v["frame"]["capsules"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(caps
        .iter()
        .any(|c| c["claim"].as_str().unwrap_or("").contains("kelpstone")));
}

#[tokio::test]
async fn metrics_endpoint_exposes_prometheus() {
    let app = app();
    let _ = app.clone().oneshot(query_req(1, "anything")).await.unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(text.contains("l5m_queries_total"));
    assert!(text.contains("l5m_query_latency_seconds_bucket"));
}

#[tokio::test]
async fn queries_are_audited_and_chain_verifies() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audit.jsonl");
    let sink = l5m_server::AuditSink {
        log: tokio::sync::Mutex::new(l5m_core::AuditLog::open(&path).unwrap()),
        path: path.clone(),
    };
    let app = app_with_audit(Some(sink));

    // Insert + two queries -> two audit records.
    let cap = serde_json::json!({
        "capsule_id":"1","tenant_id":1,"claim":"audited kelpstone","evidence":"audited kelpstone",
        "source_id":1,"valid_from":1,"observed_at":1,"last_verified_at":1,
        "context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,
        "classification":1,"poison_risk":0
    });
    let _ = app.clone().oneshot(insert_req(1, cap)).await.unwrap();
    let _ = app
        .clone()
        .oneshot(query_req(1, "audited kelpstone"))
        .await
        .unwrap();
    let _ = app
        .clone()
        .oneshot(query_req(1, "audited kelpstone"))
        .await
        .unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/audit/verify")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["intact"], true);
    assert_eq!(v["verified"], 2, "two queries should be audited");
}

#[tokio::test]
async fn scoped_keys_enforce_read_write_and_tenant_binding() {
    use l5m_server::{Scope, ScopedKey};
    let app = app_with_provider(Arc::new(l5m_server::HeaderPrincipalProvider::with_keys(
        vec![
            ScopedKey {
                secret: "ro-key".into(),
                scope: Scope::Read,
                tenant: None,
            },
            ScopedKey {
                secret: "rw-key".into(),
                scope: Scope::Write,
                tenant: Some(7), // bound: may ONLY act as tenant 7
            },
        ],
    )));

    let cap = serde_json::json!({
        "capsule_id":"1","tenant_id":1,
        "claim":"scoped kelpstone","evidence":"scoped kelpstone",
        "source_id":1,"valid_from":1,"observed_at":1,"last_verified_at":1,
        "context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,
        "classification":1,"poison_risk":0
    });

    // Read-only key: query OK, insert forbidden (403).
    let req = |key: &str, method: &str, uri: &str, body: String| {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("x-l5m-api-key", key)
            .header("x-l5m-tenant", "1")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap()
    };
    let q = serde_json::json!({"query":"scoped kelpstone"}).to_string();
    let status = app
        .clone()
        .oneshot(req("ro-key", "POST", "/v1/query", q.clone()))
        .await
        .unwrap()
        .status();
    assert_eq!(status, StatusCode::OK, "read key can query");
    let status = app
        .clone()
        .oneshot(req("ro-key", "POST", "/v1/memories", cap.to_string()))
        .await
        .unwrap()
        .status();
    assert_eq!(status, StatusCode::FORBIDDEN, "read key cannot write");

    // Tenant-bound write key: write succeeds, but lands in tenant 7 even
    // though the header (and body) claim tenant 1 — the binding wins.
    let status = app
        .clone()
        .oneshot(req("rw-key", "POST", "/v1/memories", cap.to_string()))
        .await
        .unwrap()
        .status();
    assert_eq!(status, StatusCode::CREATED);

    // Read it back as tenant 7 via the read key (header tenant 7).
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/query")
                .header("x-l5m-api-key", "ro-key")
                .header("x-l5m-tenant", "7")
                .header("content-type", "application/json")
                .body(Body::from(q.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    let v = body_json(resp).await;
    let caps = v["frame"]["capsules"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        caps.iter()
            .any(|c| c["claim"].as_str().unwrap_or("").contains("scoped")),
        "tenant-bound key's write landed in tenant 7"
    );

    // And tenant 1 (where the header pointed) must NOT have it.
    let resp = app
        .oneshot(req("ro-key", "POST", "/v1/query", q))
        .await
        .unwrap();
    let v = body_json(resp).await;
    let caps = v["frame"]["capsules"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        caps.iter()
            .all(|c| !c["claim"].as_str().unwrap_or("").contains("scoped")),
        "tenant 1 must not have the bound-key write"
    );
}

#[tokio::test]
async fn rate_limiter_returns_429_when_exceeded() {
    // burst of 2, slow refill -> the 3rd immediate query is throttled.
    let app = app_full(None, Some(l5m_server::RateLimiter::new(0.1, 2.0)));
    let s1 = app
        .clone()
        .oneshot(query_req(1, "q"))
        .await
        .unwrap()
        .status();
    let s2 = app
        .clone()
        .oneshot(query_req(1, "q"))
        .await
        .unwrap()
        .status();
    let s3 = app
        .clone()
        .oneshot(query_req(1, "q"))
        .await
        .unwrap()
        .status();
    assert_eq!(s1, StatusCode::OK);
    assert_eq!(s2, StatusCode::OK);
    assert_eq!(s3, StatusCode::TOO_MANY_REQUESTS, "3rd request throttled");
    // A different tenant has its own bucket.
    let s_other = app.oneshot(query_req(2, "q")).await.unwrap().status();
    assert_eq!(s_other, StatusCode::OK, "other tenant unaffected");
}
