// E3: observability — the store exposes Prometheus metrics that move as it's used.

use l5m_core::{MemoryStore, QueryRequest, Result, RetrievalMode};
use serde_json::json;

fn memory(id: &str, claim: &str) -> serde_json::Value {
    json!({
        "capsule_id": id, "tenant_id": 1, "claim": claim, "evidence": claim,
        "source_id": id.parse::<u64>().unwrap_or(0),
        "valid_from": 1, "observed_at": 1, "last_verified_at": 1,
        "context_mask": "0xffff", "policy_mask": "0xffff",
        "trust_level": 8, "classification": 1, "poison_risk": 0
    })
}

fn request(q: &str) -> QueryRequest {
    QueryRequest {
        query: q.to_string(),
        tenant_id: 1,
        as_of: 1000,
        context_mask: "0xffff".to_string(),
        policy_mask: "0xffff".to_string(),
        trust_floor: 0,
        max_capsules: 8,
        max_tokens: usize::MAX,
        include_supporting: false,
        include_contradictions: false,
        max_hops: 1,
        mode: RetrievalMode::L5m,
        embedding: Vec::new(),
    }
}

#[test]
fn metrics_track_inserts_and_queries() -> Result<()> {
    let mut store = MemoryStore::empty();
    store.insert_json(&memory("1", "backup retention is 35 days"))?;
    store.insert_json(&memory("2", "scanning cadence is weekly"))?;

    for _ in 0..3 {
        let _ = store.query(&request("backup retention"))?;
    }

    let text = store.metrics().render_prometheus();
    assert!(
        text.contains("l5m_queries_total 3"),
        "queries counted: {text}"
    );
    assert!(text.contains("l5m_inserts_total 2"), "inserts counted");
    assert!(text.contains("# TYPE l5m_query_latency_seconds histogram"));
    assert!(text.contains("l5m_query_latency_seconds_bucket{le=\"+Inf\"} 3"));
    assert!(text.contains("l5m_query_latency_seconds_count 3"));
    assert!(text.contains("l5m_capsules_returned_total"));
    assert_eq!(store.metrics().queries(), 3);
    Ok(())
}
