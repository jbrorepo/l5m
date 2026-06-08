// Phase 5: native dense embeddings + hybrid (lexical ⊕ dense) ranking.
// Proves: embeddings round-trip through the segment; the dense signal is
// load-bearing (changes the top result); and dense similarity never bypasses the
// security gates.

use std::fs;

use l5m_core::{compile_segment, retrieve, CompileOptions, MemoryProbe, Result, Segment};
use tempfile::tempdir;

// 5 authorized capsules + 1 unauthorized (tenant 2). D1 is the best *lexical*
// match for the query; T is a weak lexical match but the best *dense* match.
const CORPUS: &str = r#"[
  {"capsule_id":"1","tenant_id":1,"claim":"alpha beta gamma delta epsilon","evidence":"alpha beta gamma delta epsilon","source_id":1,"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,"classification":1,"poison_risk":0,"embedding":[0.0,0.0,0.0,0.0,-1.0]},
  {"capsule_id":"2","tenant_id":1,"claim":"alpha solo","evidence":"alpha solo","source_id":2,"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,"classification":1,"poison_risk":0,"embedding":[0.0,0.0,0.0,0.0,1.0]},
  {"capsule_id":"3","tenant_id":1,"claim":"beta solo","evidence":"beta solo","source_id":3,"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,"classification":1,"poison_risk":0,"embedding":[1.0,0.0,0.0,0.0,0.0]},
  {"capsule_id":"4","tenant_id":1,"claim":"gamma solo","evidence":"gamma solo","source_id":4,"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,"classification":1,"poison_risk":0,"embedding":[0.0,1.0,0.0,0.0,0.0]},
  {"capsule_id":"5","tenant_id":1,"claim":"delta solo","evidence":"delta solo","source_id":5,"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,"classification":1,"poison_risk":0,"embedding":[0.0,0.0,1.0,0.0,0.0]},
  {"capsule_id":"6","tenant_id":2,"claim":"alpha beta gamma delta epsilon","evidence":"alpha beta gamma delta epsilon","source_id":6,"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,"classification":1,"poison_risk":0,"embedding":[0.0,0.0,0.0,0.0,1.0]}
]"#;

fn build() -> Result<(tempfile::TempDir, Segment)> {
    let dir = tempdir()?;
    let input = dir.path().join("in.json");
    let output = dir.path().join("seg.segment");
    fs::write(&input, CORPUS)?;
    compile_segment(CompileOptions {
        input_json: input,
        output_segment: output.clone(),
        epoch: 1,
    })?;
    Ok((dir, Segment::open(output)?))
}

fn probe() -> MemoryProbe {
    let mut p = MemoryProbe::build("alpha beta gamma delta epsilon", 1, 1000, 0xffff, 0xffff, 0);
    p.max_capsules = 8;
    p
}

#[test]
fn embeddings_round_trip_through_segment() -> Result<()> {
    let (_d, seg) = build()?;
    let c1 = seg.capsule_by_id(1).expect("capsule 1");
    assert_eq!(c1.embedding, vec![0.0, 0.0, 0.0, 0.0, -1.0]);
    let c2 = seg.capsule_by_id(2).expect("capsule 2");
    assert_eq!(c2.embedding, vec![0.0, 0.0, 0.0, 0.0, 1.0]);
    Ok(())
}

#[test]
fn dense_signal_is_load_bearing() -> Result<()> {
    let (_d, seg) = build()?;

    // Pure lexical: the all-terms capsule (id 1) wins.
    let lexical = retrieve(&seg, &probe())?;
    assert_eq!(
        lexical.capsules[0].capsule_id, 1,
        "lexical top-1 should be id 1"
    );

    // Hybrid: probe embedding matches the weak-lexical capsule (id 2); fusion
    // promotes it to the top.
    let mut p = probe();
    p.embedding = vec![0.0, 0.0, 0.0, 0.0, 1.0];
    let hybrid = retrieve(&seg, &p)?;
    assert_eq!(
        hybrid.capsules[0].capsule_id, 2,
        "dense fusion should promote the embedding-matched capsule to top-1"
    );
    Ok(())
}

#[test]
fn dense_match_cannot_bypass_tenant_gate() -> Result<()> {
    let (_d, seg) = build()?;
    // capsule 6 is tenant 2 with a perfect dense match for a tenant-1 probe.
    let mut p = probe();
    p.embedding = vec![0.0, 0.0, 0.0, 0.0, 1.0];
    let frame = retrieve(&seg, &p)?;
    assert!(
        frame.capsules.iter().all(|c| c.capsule_id != 6),
        "tenant-2 capsule leaked via dense similarity"
    );
    Ok(())
}
