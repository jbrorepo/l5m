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
        principal: Arc::new(HeaderPrincipalProvider {
            api_key: Some("secret".to_string()),
        }),
        audit,
        rate_limiter,
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
