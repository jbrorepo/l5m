// Adversarial security-gate validation.
//
// Each unauthorized capsule below is a *perfect* lexical/semantic match for the
// query — identical wording to an authorized decoy. If the security gates did
// NOT run before scoring, these capsules would rank #1 and leak. The test
// asserts they never appear in the frame, while the authorized capsule does.
//
// This is stronger than asserting on incidental non-matches: the secret is the
// single best-matching document in the segment.

use std::fs;

use l5m_core::{
    compile_segment, retrieve, retrieve::retrieve_with_config, retrieve::RetrievalConfig,
    CompileOptions, MemoryProbe, Result, Segment,
};
use tempfile::tempdir;

const QUERY: &str = "What is the production database backup retention window codename Falcon?";
const SECRET: &str = "FALCON-RETENTION-SECRET-9931";

fn segment_from(json: &str) -> Result<(tempfile::TempDir, Segment)> {
    let dir = tempdir()?;
    let input = dir.path().join("in.json");
    let output = dir.path().join("seg.segment");
    fs::write(&input, json)?;
    compile_segment(CompileOptions {
        input_json: input,
        output_segment: output.clone(),
        epoch: 1,
    })?;
    Ok((dir, Segment::open(output)?))
}

/// Build two capsules with IDENTICAL claim/evidence text (so identical semantic
/// fingerprint and lexical match). `authorized` is a clean tenant-1 capsule;
/// `attacker_overrides` injects the property that should get it gated out.
fn two_capsule_json(attacker_overrides: &str) -> String {
    let body = format!("production database backup retention window codename Falcon {SECRET}");
    format!(
        r#"[
          {{
            "capsule_id": "1",
            "tenant_id": 1,
            "claim": "Authorized decoy: {body}",
            "evidence": "Authorized tenant-one record. {body}",
            "source_id": 10,
            "valid_from": 1,
            "observed_at": 1,
            "last_verified_at": 1,
            "context_mask": "0x1",
            "policy_mask": "0xffff",
            "trust_level": 8,
            "classification": 1,
            "poison_risk": 0
          }},
          {{
            "capsule_id": "2",
            "claim": "ATTACKER SECRET: {body}",
            "evidence": "Unauthorized record. {body}",
            "source_id": 20,
            "observed_at": 1,
            "last_verified_at": 1,
            "poison_risk": 0,
            {attacker_overrides}
          }}
        ]"#
    )
}

fn probe() -> MemoryProbe {
    // tenant 1, as_of=1000, context 0x1, policy 0x1, trust_floor 5
    let mut p = MemoryProbe::build(QUERY, 1, 1000, 0x1, 0x1, 5);
    p.max_capsules = 8;
    p
}

fn assert_secret_blocked_decoy_present(json: &str, label: &str) -> Result<()> {
    let (_d, seg) = segment_from(json)?;
    let frame = retrieve(&seg, &probe())?;
    let leaked = frame
        .capsules
        .iter()
        .any(|c| c.claim.contains(SECRET) && c.claim.contains("ATTACKER"));
    let decoy_present = frame
        .capsules
        .iter()
        .any(|c| c.claim.contains("Authorized decoy"));
    assert!(
        !leaked,
        "[{label}] SECURITY FAILURE: attacker capsule leaked"
    );
    assert!(
        decoy_present,
        "[{label}] positive control failed: authorized decoy should have matched"
    );
    Ok(())
}

#[test]
fn tenant_gate_blocks_perfect_match_secret() -> Result<()> {
    // Attacker is tenant 2 — different tenant, otherwise fully authorized + valid.
    let json = two_capsule_json(
        r#""tenant_id": 2, "valid_from": 1, "context_mask": "0x1", "policy_mask": "0xffff", "trust_level": 10, "classification": 1"#,
    );
    assert_secret_blocked_decoy_present(&json, "tenant")
}

#[test]
fn policy_gate_blocks_perfect_match_secret() -> Result<()> {
    // Right tenant, but policy_mask 0x8 does not intersect caller policy 0x1.
    let json = two_capsule_json(
        r#""tenant_id": 1, "valid_from": 1, "context_mask": "0x1", "policy_mask": "0x8", "trust_level": 10, "classification": 1"#,
    );
    assert_secret_blocked_decoy_present(&json, "policy")
}

#[test]
fn trust_gate_blocks_perfect_match_secret() -> Result<()> {
    // Right tenant/policy/context/time, but trust_level 2 < floor 5.
    let json = two_capsule_json(
        r#""tenant_id": 1, "valid_from": 1, "context_mask": "0x1", "policy_mask": "0xffff", "trust_level": 2, "classification": 1"#,
    );
    assert_secret_blocked_decoy_present(&json, "trust")
}

#[test]
fn temporal_gate_blocks_expired_perfect_match_secret() -> Result<()> {
    // Expired: valid_until=500 < as_of=1000.
    let json = two_capsule_json(
        r#""tenant_id": 1, "valid_from": 1, "valid_until": 500, "context_mask": "0x1", "policy_mask": "0xffff", "trust_level": 10, "classification": 1"#,
    );
    assert_secret_blocked_decoy_present(&json, "temporal-expired")
}

#[test]
fn temporal_gate_blocks_future_perfect_match_secret() -> Result<()> {
    // Not yet valid: valid_from=5000 > as_of=1000.
    let json = two_capsule_json(
        r#""tenant_id": 1, "valid_from": 5000, "context_mask": "0x1", "policy_mask": "0xffff", "trust_level": 10, "classification": 1"#,
    );
    assert_secret_blocked_decoy_present(&json, "temporal-future")
}

#[test]
fn context_gate_blocks_perfect_match_secret() -> Result<()> {
    // context_mask 0x2 does not intersect probe context 0x1.
    let json = two_capsule_json(
        r#""tenant_id": 1, "valid_from": 1, "context_mask": "0x2", "policy_mask": "0xffff", "trust_level": 10, "classification": 1"#,
    );
    assert_secret_blocked_decoy_present(&json, "context")
}

/// The two-stage candidate cap must never let an unauthorized capsule through,
/// even when the cap is smaller than the corpus. Build many authorized fillers
/// plus one perfect-match attacker in a different tenant, then score with a
/// cap of 1: gates run before the cap, so the attacker is never a candidate.
#[test]
fn candidate_cap_does_not_bypass_gates() -> Result<()> {
    let body = "production database backup retention window codename Falcon";
    let mut entries = Vec::new();
    for i in 1..=40 {
        entries.push(format!(
            r#"{{ "capsule_id":"{i}","tenant_id":1,"claim":"authorized filler {i} {body}","evidence":"{body} filler {i}","source_id":{i},"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0x1","policy_mask":"0x1","trust_level":8,"classification":1,"poison_risk":0 }}"#
        ));
    }
    // Attacker: tenant 2, byte-identical perfect match for the query + secret.
    entries.push(format!(
        r#"{{ "capsule_id":"999","tenant_id":2,"claim":"ATTACKER {body} {SECRET}","evidence":"{body} {SECRET}","source_id":999,"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0x1","policy_mask":"0x1","trust_level":10,"classification":1,"poison_risk":0 }}"#
    ));
    let json = format!("[{}]", entries.join(","));
    let (_d, seg) = segment_from(&json)?;

    let cfg = RetrievalConfig {
        semantic_hamming_threshold: 256,
        max_scored_candidates: 1,   // force the cap path
        ann_candidate_threshold: 0, // force the sublinear LSH path too
        embed_rrf_k: 60.0,
    };
    let frame = retrieve_with_config(&seg, &probe(), &cfg)?;
    assert!(
        frame.capsules.iter().all(|c| !c.claim.contains(SECRET)),
        "candidate cap leaked an unauthorized capsule"
    );
    assert!(
        frame
            .capsules
            .iter()
            .all(|c| { seg.capsule_by_id(c.capsule_id).map(|s| s.tenant_id) == Some(1) }),
        "returned a non-tenant-1 capsule under the cap"
    );
    Ok(())
}

/// Multi-tenant correctness: with many tenants sharing one segment, a probe for
/// tenant T must return only tenant-T capsules and still find tenant T's answer
/// among same-text distractors owned by other tenants.
#[test]
fn multi_tenant_scan_is_isolated_and_complete() -> Result<()> {
    let body = "production database backup retention window codename Falcon";
    let mut entries = Vec::new();
    // 60 tenants, each with one same-text capsule; only tenant 7 also carries the
    // distinctive answer token.
    for t in 1..=60u64 {
        let id = t;
        entries.push(format!(
            r#"{{ "capsule_id":"{id}","tenant_id":{t},"claim":"tenant {t} {body}","evidence":"{body}","source_id":{id},"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0x1","policy_mask":"0x1","trust_level":8,"classification":1,"poison_risk":0 }}"#
        ));
    }
    entries.push(format!(
        r#"{{ "capsule_id":"777","tenant_id":7,"claim":"tenant 7 answer {body} {SECRET}","evidence":"{body} {SECRET}","source_id":777,"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0x1","policy_mask":"0x1","trust_level":8,"classification":1,"poison_risk":0 }}"#
    ));
    let json = format!("[{}]", entries.join(","));
    let (_d, seg) = segment_from(&json)?;

    // Probe as tenant 7.
    let mut p = MemoryProbe::build(QUERY, 7, 1000, 0x1, 0x1, 5);
    p.max_capsules = 8;
    let frame = retrieve(&seg, &p)?;

    assert!(
        !frame.capsules.is_empty(),
        "tenant 7 should retrieve its capsules"
    );
    for c in &frame.capsules {
        let src = seg.capsule_by_id(c.capsule_id).expect("exists");
        assert_eq!(
            src.tenant_id, 7,
            "cross-tenant capsule leaked into tenant-7 results"
        );
    }
    // tenant 7's distinctive answer is found.
    assert!(
        frame.capsules.iter().any(|c| c.claim.contains(SECRET)),
        "tenant 7's own answer should be retrievable"
    );
    Ok(())
}

/// Universal invariant: regardless of gate, EVERY returned capsule must satisfy
/// every hard gate. Throw all attackers in at once.
#[test]
fn no_returned_capsule_violates_any_hard_gate() -> Result<()> {
    let body = "production database backup retention window codename Falcon";
    let json = format!(
        r#"[
          {{ "capsule_id":"1","tenant_id":1,"claim":"ok {body}","evidence":"{body}","source_id":1,"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0x1","policy_mask":"0x1","trust_level":8,"classification":1,"poison_risk":0 }},
          {{ "capsule_id":"2","tenant_id":2,"claim":"x {body}","evidence":"{body}","source_id":2,"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0x1","policy_mask":"0x1","trust_level":8,"classification":1,"poison_risk":0 }},
          {{ "capsule_id":"3","tenant_id":1,"claim":"x {body}","evidence":"{body}","source_id":3,"valid_from":1,"valid_until":500,"observed_at":1,"last_verified_at":1,"context_mask":"0x1","policy_mask":"0x1","trust_level":8,"classification":1,"poison_risk":0 }},
          {{ "capsule_id":"4","tenant_id":1,"claim":"x {body}","evidence":"{body}","source_id":4,"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0x2","policy_mask":"0x1","trust_level":8,"classification":1,"poison_risk":0 }},
          {{ "capsule_id":"5","tenant_id":1,"claim":"x {body}","evidence":"{body}","source_id":5,"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0x1","policy_mask":"0x8","trust_level":8,"classification":1,"poison_risk":0 }},
          {{ "capsule_id":"6","tenant_id":1,"claim":"x {body}","evidence":"{body}","source_id":6,"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0x1","policy_mask":"0x1","trust_level":2,"classification":1,"poison_risk":0 }}
        ]"#
    );
    let (_d, seg) = segment_from(&json)?;
    let p = probe(); // tenant 1, as_of 1000, context 0x1, policy 0x1, trust_floor 5
    let frame = retrieve(&seg, &p)?;
    assert!(
        !frame.capsules.is_empty(),
        "authorized capsule 1 should be returned"
    );
    for c in &frame.capsules {
        let src = seg.capsule_by_id(c.capsule_id).expect("exists");
        assert_eq!(src.tenant_id, p.tenant_id, "tenant gate breached");
        assert_ne!(
            src.context_mask & p.context_mask,
            0,
            "context gate breached"
        );
        assert_ne!(
            src.policy_mask & p.caller_policy_mask,
            0,
            "policy gate breached"
        );
        assert!(src.valid_from <= p.as_of, "temporal(from) gate breached");
        assert!(
            src.valid_until.is_none_or(|u| u >= p.as_of),
            "temporal(until) gate breached"
        );
        assert!(src.trust_level >= p.trust_floor, "trust gate breached");
    }
    // Only capsule 1 is fully authorized.
    assert_eq!(frame.capsules.len(), 1);
    assert_eq!(frame.capsules[0].capsule_id, 1);
    Ok(())
}
