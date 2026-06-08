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
