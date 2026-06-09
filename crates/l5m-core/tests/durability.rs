// E5: durable mutable layer. Acknowledged writes must survive a restart, and a
// checkpoint must fold them into a base segment while staying recoverable.

use std::fs;

use l5m_core::{MemoryStore, QueryRequest, Result, RetrievalMode};
use serde_json::json;
use tempfile::tempdir;

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

fn finds(store: &MemoryStore, q: &str, needle: &str) -> Result<bool> {
    let resp = store.query(&request(q))?;
    Ok(resp.frame.capsules.iter().any(|c| c.claim.contains(needle)))
}

#[test]
fn writes_survive_restart_via_wal() -> Result<()> {
    let dir = tempdir()?;
    let wal = dir.path().join("l5m.wal");

    // Session 1: write then drop the store (simulating a process exit).
    {
        let mut store = MemoryStore::open_durable(Vec::<&str>::new(), &wal)?;
        store.insert_json(&memory("1", "violet passphrase kelpstone"))?;
        store.insert_json(&memory("2", "scarlet driftwood"))?;
        store.delete(2)?;
    }

    // Session 2: reopen from the SAME wal — recovered.
    let store = MemoryStore::open_durable(Vec::<&str>::new(), &wal)?;
    assert!(
        finds(&store, "violet passphrase", "kelpstone")?,
        "insert recovered"
    );
    assert!(
        !finds(&store, "scarlet driftwood", "driftwood")?,
        "delete recovered (tombstone persisted)"
    );
    Ok(())
}

#[test]
fn checkpoint_persists_and_truncates_wal() -> Result<()> {
    let dir = tempdir()?;
    let wal = dir.path().join("l5m.wal");
    let checkpoint = dir.path().join("base.segment");

    {
        let mut store = MemoryStore::open_durable(Vec::<&str>::new(), &wal)?;
        store.insert_json(&memory("1", "keep me kelpstone"))?;
        store.insert_json(&memory("2", "drop me driftwood"))?;
        store.delete(2)?;
        store.compact_to(&checkpoint)?;
        // WAL truncated after checkpoint.
        assert_eq!(fs::read_to_string(&wal)?.trim(), "", "wal truncated");
    }

    // Reopen from the checkpoint base + (now-empty) wal.
    let store = MemoryStore::open_durable([checkpoint.to_str().unwrap()], &wal)?;
    assert!(finds(&store, "keep me", "kelpstone")?);
    assert!(!finds(&store, "drop me", "driftwood")?);
    Ok(())
}

#[test]
fn crash_after_checkpoint_before_truncate_is_consistent() -> Result<()> {
    // Simulate the dangerous window: checkpoint written, but WAL still holds the
    // ops (as if we crashed before truncation). Replaying on top must converge.
    let dir = tempdir()?;
    let wal = dir.path().join("l5m.wal");
    let checkpoint = dir.path().join("base.segment");

    {
        let mut store = MemoryStore::open_durable(Vec::<&str>::new(), &wal)?;
        store.insert_json(&memory("1", "alpha kelpstone"))?;
        // Manually checkpoint WITHOUT truncating the wal (compile then leave wal).
        l5m_core::compiler::compile_capsules(
            &checkpoint,
            vec![l5m_core::compiler::capsule_from_json(&memory(
                "1",
                "alpha kelpstone",
            ))?],
            1,
        )?;
    }
    // Recover from checkpoint + non-empty wal: insert replays idempotently.
    let store = MemoryStore::open_durable([checkpoint.to_str().unwrap()], &wal)?;
    let resp = store.query(&request("alpha kelpstone"))?;
    let hits = resp
        .frame
        .capsules
        .iter()
        .filter(|c| c.claim.contains("kelpstone"))
        .count();
    assert_eq!(hits, 1, "no duplicate after replay-on-top; got {hits}");
    Ok(())
}
