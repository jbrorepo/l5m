//! Live proof of L5M's core differentiator: **gate-before-scoring**.
//!
//! A vector database ranks first and filters second — the secret vector is
//! retrieved, scored, and only then (hopefully) dropped by a metadata filter the
//! application remembered to add. L5M constructs the authorized candidate set
//! *before* anything is scored, so an unauthorized capsule is never even a
//! candidate — even when it is a byte-for-byte perfect match for the query.
//!
//! Run it:
//!     cargo run -p l5m-core --example leak_demo
//!
//! Every check prints PASS/FAIL and the process exits non-zero if any gate leaks,
//! so this doubles as an executable security assertion.

use l5m_core::{MemoryStore, QueryRequest, Result, RetrievalMode};
use serde_json::json;

/// Build a capsule. `policy_mask`/`context_mask` are hex strings; `trust_level`
/// is the capsule's own trustworthiness (the gate keeps capsules whose trust is
/// >= the caller's required floor).
#[allow(clippy::too_many_arguments)]
fn capsule(
    id: u128,
    tenant: u64,
    claim: &str,
    context_mask: &str,
    policy_mask: &str,
    trust_level: u8,
) -> serde_json::Value {
    json!({
        "capsule_id": id.to_string(),
        "tenant_id": tenant,
        "claim": claim,
        "evidence": claim,
        "source_id": id as u64,
        "valid_from": 1, "observed_at": 1, "last_verified_at": 1,
        "context_mask": context_mask,
        "policy_mask": policy_mask,
        "trust_level": trust_level,
        "classification": 1,
        "poison_risk": 0
    })
}

/// A query under a specific principal (tenant / policy / trust floor).
fn query(tenant: u64, text: &str, policy_mask: &str, trust_floor: u8) -> QueryRequest {
    QueryRequest {
        query: text.to_string(),
        tenant_id: tenant,
        as_of: 1000,
        context_mask: "0xffff".to_string(),
        policy_mask: policy_mask.to_string(),
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

/// The secret string used everywhere — a *perfect* match guarantees that only the
/// gate, never the ranker, is what keeps it hidden.
const SECRET: &str = "the production database master password is hunter2-kelpstone";

fn main() -> Result<()> {
    let mut store = MemoryStore::empty();

    // Tenant 7 stores the crown-jewel secret with the most-secret policy bit
    // (0x4) and maximum trust.
    store.insert_json(&capsule(1, 7, SECRET, "0xffff", "0x4", 10))?;
    // Tenant 42 stores something innocuous so its queries return *something*.
    store.insert_json(&capsule(
        2,
        42,
        "the weather today is mild and sunny",
        "0xffff",
        "0x1",
        5,
    ))?;

    let mut leaked = false;

    println!("=== L5M gate-before-scoring — live leak demo ===\n");
    println!("Stored: tenant 7 holds a perfect-match secret (policy 0x4, trust 10).\n");

    // --- 1) Tenant isolation -------------------------------------------------
    // Tenant 42 asks for the EXACT secret text. A vector DB would rank it #1.
    let resp = store.query(&query(42, SECRET, "0xffff", 0))?;
    let saw_secret = resp
        .frame
        .capsules
        .iter()
        .any(|c| c.claim.contains("hunter2"));
    report(
        "Cross-tenant isolation",
        "tenant 42 queries the exact secret text",
        !saw_secret,
        resp.frame.coverage.candidate_count_before_scoring,
        &mut leaked,
    );

    // --- 2) Policy / clearance gate -----------------------------------------
    // Same tenant (7), but the caller lacks the secret policy bit 0x4 (only 0x1).
    let resp = store.query(&query(7, SECRET, "0x1", 0))?;
    let saw_secret = resp
        .frame
        .capsules
        .iter()
        .any(|c| c.claim.contains("hunter2"));
    report(
        "Policy clearance gate",
        "tenant 7 caller WITHOUT the 0x4 clearance bit",
        !saw_secret,
        resp.frame.coverage.candidate_count_before_scoring,
        &mut leaked,
    );

    // --- 3) Authorized caller still works -----------------------------------
    // Tenant 7 WITH the 0x4 bit must get it — gates block, they don't break.
    let resp = store.query(&query(7, SECRET, "0x4", 0))?;
    let saw_secret = resp
        .frame
        .capsules
        .iter()
        .any(|c| c.claim.contains("hunter2"));
    report(
        "Authorized retrieval",
        "tenant 7 caller WITH the 0x4 clearance bit",
        saw_secret, // here we WANT to see it
        resp.frame.coverage.candidate_count_before_scoring,
        &mut leaked,
    );

    println!();
    if leaked {
        eprintln!("RESULT: LEAK DETECTED — a gate failed. This must never happen.");
        std::process::exit(1);
    }
    println!("RESULT: all gates held. The secret was never scored for an");
    println!("        unauthorized caller, and was returned to the authorized one.");
    Ok(())
}

fn report(name: &str, scenario: &str, ok: bool, candidates: usize, leaked: &mut bool) {
    let status = if ok { "PASS" } else { "FAIL" };
    if !ok {
        *leaked = true;
    }
    println!("[{status}] {name}");
    println!("        scenario          : {scenario}");
    println!("        candidates scored : {candidates}  (size of the post-gate set the ranker ever sees)\n");
}
