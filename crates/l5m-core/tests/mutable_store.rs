// Phase 4: the mutable / real-time memory layer. Proves runtime insert, update,
// delete (tombstone), tenant isolation in the live delta, security gating of
// freshly written memories, and compaction — all sharing the same retrieval
// path as compiled base segments.

use l5m_core::{MemoryStore, QueryRequest, Result, RetrievalMode};
use serde_json::json;

fn memory(id: &str, tenant: u64, claim: &str, trust: u8) -> serde_json::Value {
    json!({
        "capsule_id": id,
        "tenant_id": tenant,
        "claim": claim,
        "evidence": claim,
        "source_id": id.parse::<u64>().unwrap_or(0),
        "valid_from": 1,
        "observed_at": 1,
        "last_verified_at": 1,
        "context_mask": "0xffff",
        "policy_mask": "0xffff",
        "trust_level": trust,
        "classification": 1,
        "poison_risk": 0
    })
}

fn request(query: &str, tenant: u64, trust_floor: u8) -> QueryRequest {
    QueryRequest {
        query: query.to_string(),
        tenant_id: tenant,
        as_of: 1000,
        context_mask: "0xffff".to_string(),
        policy_mask: "0xffff".to_string(),
        trust_floor,
        max_capsules: 8,
        max_tokens: usize::MAX,
        include_supporting: false,
        include_contradictions: false,
        max_hops: 1,
        mode: RetrievalMode::L5m,
        embedding: Vec::new(),
    }
}

fn found(store: &MemoryStore, query: &str, tenant: u64, needle: &str) -> Result<bool> {
    let resp = store.query(&request(query, tenant, 0))?;
    Ok(resp.frame.capsules.iter().any(|c| c.claim.contains(needle)))
}

#[test]
fn insert_then_query_finds_new_memory() -> Result<()> {
    let mut store = MemoryStore::empty();
    store.insert_json(&memory(
        "1",
        1,
        "The rare violet passphrase is kelpstone.",
        8,
    ))?;
    assert!(found(
        &store,
        "what is the rare violet passphrase",
        1,
        "kelpstone"
    )?);
    Ok(())
}

#[test]
fn delta_respects_tenant_isolation() -> Result<()> {
    let mut store = MemoryStore::empty();
    store.insert_json(&memory("1", 1, "Tenant one secret is kelpstone.", 8))?;
    // Same query as a different tenant must not see it.
    assert!(found(&store, "tenant secret kelpstone", 1, "kelpstone")?);
    assert!(!found(&store, "tenant secret kelpstone", 2, "kelpstone")?);
    Ok(())
}

#[test]
fn delta_respects_trust_gate() -> Result<()> {
    let mut store = MemoryStore::empty();
    store.insert_json(&memory("1", 1, "Low trust rumor about kelpstone.", 2))?;
    // trust_floor 5 excludes the trust-2 memory.
    let resp = store.query(&request("rumor kelpstone", 1, 5))?;
    assert!(resp
        .frame
        .capsules
        .iter()
        .all(|c| !c.claim.contains("kelpstone")));
    // trust_floor 0 sees it.
    assert!(found(&store, "rumor kelpstone", 1, "kelpstone")?);
    Ok(())
}

#[test]
fn delete_tombstones_memory() -> Result<()> {
    let mut store = MemoryStore::empty();
    store.insert_json(&memory("1", 1, "Delete me: kelpstone fact.", 8))?;
    assert!(found(&store, "kelpstone fact", 1, "kelpstone")?);
    store.delete(1)?;
    assert!(!found(&store, "kelpstone fact", 1, "kelpstone")?);
    Ok(())
}

#[test]
fn compaction_preserves_live_memories_and_drops_tombstones() -> Result<()> {
    let mut store = MemoryStore::empty();
    store.insert_json(&memory("1", 1, "Keep: violet kelpstone.", 8))?;
    store.insert_json(&memory("2", 1, "Drop: scarlet driftwood.", 8))?;
    store.delete(2)?;

    store.compact()?;
    assert_eq!(
        store.delta_len(),
        0,
        "delta should be empty after compaction"
    );

    assert!(found(&store, "violet kelpstone", 1, "kelpstone")?);
    assert!(!found(&store, "scarlet driftwood", 1, "driftwood")?);
    Ok(())
}

#[test]
fn active_buffer_seals_into_runs_and_all_tiers_stay_queryable() -> Result<()> {
    // Small threshold so we cross several seal boundaries quickly. Each capsule
    // carries a distinctive token ("zorbixNN") so retrieval is unambiguous —
    // this test exercises the tiering, not ranking among near-duplicates.
    let mut store = MemoryStore::empty().with_seal_threshold(8);
    for i in 0..40u64 {
        store.insert_json(&memory(
            &i.to_string(),
            1,
            &format!("zorbix{i} secret fact"),
            8,
        ))?;
    }
    // The active buffer was bounded and frozen into immutable runs.
    assert!(
        store.sealed_run_count() >= 3,
        "expected several sealed runs"
    );
    assert!(store.delta_len() <= 8, "active buffer stays bounded");

    // Capsules from an early run, a middle run, and the active buffer are all
    // retrievable — gating + scoring run identically across every tier.
    assert!(found(&store, "zorbix0", 1, "zorbix0 ")?);
    assert!(found(&store, "zorbix19", 1, "zorbix19 ")?);
    assert!(found(&store, "zorbix39", 1, "zorbix39 ")?);
    // Tenant isolation still holds across sealed runs.
    assert!(!found(&store, "zorbix0", 2, "zorbix0")?);
    Ok(())
}

#[test]
fn update_and_reinsert_win_across_seal_boundaries() -> Result<()> {
    let mut store = MemoryStore::empty().with_seal_threshold(4);
    // id 1 written early (will be sealed away by later writes).
    store.insert_json(&memory("1", 1, "quasar original payload", 8))?;
    // Fill past several seal boundaries so id 1 lands in a sealed run. Distinct
    // vocabulary so each is individually retrievable.
    for i in 10..30u64 {
        store.insert_json(&memory(
            &i.to_string(),
            1,
            &format!("nimbus{i} filler payload"),
            8,
        ))?;
    }
    assert!(store.sealed_run_count() >= 1);

    // Delete id 1 — the tombstone must mask it even though its only copy lives in
    // a sealed run now.
    store.delete(1)?;
    assert!(
        !found(&store, "quasar", 1, "quasar")?,
        "sealed copy is hidden"
    );

    // Reinsert id 1 with new content — newest-tier-wins means the fresh version
    // resolves, not the stale sealed one.
    store.insert_json(&memory("1", 1, "quasar restored payload", 8))?;
    assert!(
        found(&store, "quasar", 1, "restored")?,
        "fresh version wins"
    );
    assert!(
        !found(&store, "quasar", 1, "original")?,
        "stale version suppressed"
    );

    // Compaction folds every tier into one base and clears the delta entirely.
    store.compact()?;
    assert_eq!(store.delta_len(), 0);
    assert_eq!(store.sealed_run_count(), 0);
    assert!(
        found(&store, "quasar", 1, "restored")?,
        "survives compaction"
    );
    assert!(found(&store, "nimbus15", 1, "nimbus15")?);
    Ok(())
}

// Real proof of the incremental-delta win. Run with:
//   cargo test -p l5m-core --release --test mutable_store -- --ignored --nocapture
// A bounded active buffer makes inserts amortized O(1); an effectively-unbounded
// buffer (seal_threshold = N, i.e. the old "rebuild the whole delta each write"
// behavior) is O(N^2). At N this gap is dramatic.
#[test]
#[ignore = "timing/perf demonstration; run explicitly with --ignored --release"]
fn bounded_delta_is_dramatically_faster_than_unbounded() -> Result<()> {
    use std::time::Instant;
    let n = 4000u64;

    let mut unbounded = MemoryStore::empty().with_seal_threshold(usize::MAX);
    let t0 = Instant::now();
    for i in 0..n {
        unbounded.insert_json(&memory(&i.to_string(), 1, &format!("rec{i} payload"), 8))?;
    }
    let unbounded_ms = t0.elapsed().as_secs_f64() * 1e3;

    let mut bounded = MemoryStore::empty().with_seal_threshold(1024);
    let t1 = Instant::now();
    for i in 0..n {
        bounded.insert_json(&memory(&i.to_string(), 1, &format!("rec{i} payload"), 8))?;
    }
    let bounded_ms = t1.elapsed().as_secs_f64() * 1e3;

    println!(
        "insert {n}: unbounded(O(N^2)) {unbounded_ms:.1} ms vs bounded(O(N)) {bounded_ms:.1} ms \
         -> {:.1}x faster",
        unbounded_ms / bounded_ms
    );
    assert!(
        bounded_ms * 2.0 < unbounded_ms,
        "bounded delta should be much faster: {bounded_ms:.1} ms vs {unbounded_ms:.1} ms"
    );
    Ok(())
}

#[test]
fn reinsert_after_delete_is_visible_again() -> Result<()> {
    let mut store = MemoryStore::empty();
    store.insert_json(&memory("1", 1, "kelpstone original.", 8))?;
    store.delete(1)?;
    assert!(!found(&store, "kelpstone", 1, "kelpstone")?);
    // Re-inserting the same id clears the tombstone.
    store.insert_json(&memory("1", 1, "kelpstone restored.", 8))?;
    assert!(found(&store, "kelpstone", 1, "restored")?);
    Ok(())
}
