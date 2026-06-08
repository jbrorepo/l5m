use std::fs;

use l5m_core::{
    compile_segment, CompileOptions, MemoryStore, QueryRequest, Result, RetrievalMode, Segment,
};
use tempfile::tempdir;

fn build_segment(json: &str) -> Result<(tempfile::TempDir, std::path::PathBuf)> {
    let dir = tempdir()?;
    let input = dir.path().join("input.json");
    let output = dir.path().join("test.segment");
    fs::write(&input, json)?;
    compile_segment(CompileOptions {
        input_json: input,
        output_segment: output.clone(),
        epoch: 7,
    })?;
    Ok((dir, output))
}

#[test]
fn memory_store_query_matches_direct_l5m_retrieval() -> Result<()> {
    let (_dir, segment_path) = build_segment(
        r#"[
          {
            "capsule_id": "1",
            "tenant_id": 1,
            "claim": "Production backups are retained for 35 days.",
            "evidence": "Approved backup policy.",
            "source_id": 10,
            "valid_from": 1,
            "observed_at": 1,
            "last_verified_at": 1,
            "context_mask": "0x1",
            "policy_mask": "0xffff",
            "trust_level": 8,
            "classification": 1,
            "poison_risk": 0
          }
        ]"#,
    )?;
    let store = MemoryStore::open_segments([segment_path.clone()])?;
    let request = QueryRequest {
        query: "How long are production backups retained?".to_string(),
        tenant_id: 1,
        as_of: 10,
        context_mask: "0x1".to_string(),
        policy_mask: "0xffff".to_string(),
        trust_floor: 4,
        max_capsules: 8,
        max_tokens: 1024,
        include_supporting: false,
        include_contradictions: false,
        max_hops: 1,
        mode: RetrievalMode::L5m,
        embedding: Vec::new(),
    };

    let response = store.query(&request)?;
    let segment = Segment::open(segment_path)?;
    let direct = l5m_core::retrieve(&segment, &request.to_probe()?)?;

    assert_eq!(
        response.frame.capsules[0].capsule_id,
        direct.capsules[0].capsule_id
    );
    assert_eq!(response.mode, RetrievalMode::L5m);
    assert_eq!(response.segment_count, 1);
    assert!(response.config_hash.iter().any(|byte| *byte != 0));
    Ok(())
}

#[test]
fn rrf_fusion_keeps_hard_gate_failures_out_of_results() -> Result<()> {
    let (_dir, segment_path) = build_segment(
        r#"[
          {
            "capsule_id": "1",
            "tenant_id": 1,
            "claim": "Allowed production backup policy is 35 days.",
            "evidence": "Approved policy.",
            "source_id": 10,
            "valid_from": 1,
            "observed_at": 1,
            "last_verified_at": 1,
            "context_mask": "0x1",
            "policy_mask": "0x1",
            "trust_level": 8,
            "classification": 1,
            "poison_risk": 0
          },
          {
            "capsule_id": "2",
            "tenant_id": 2,
            "claim": "Forbidden tenant two production backup policy is forever.",
            "evidence": "This should never be visible to tenant one.",
            "source_id": 20,
            "valid_from": 1,
            "observed_at": 1,
            "last_verified_at": 1,
            "context_mask": "0x1",
            "policy_mask": "0x1",
            "trust_level": 9,
            "classification": 1,
            "poison_risk": 0
          }
        ]"#,
    )?;
    let store = MemoryStore::open_segments([segment_path])?;
    let request = QueryRequest {
        query: "production backup policy forever".to_string(),
        tenant_id: 1,
        as_of: 10,
        context_mask: "0x1".to_string(),
        policy_mask: "0x1".to_string(),
        trust_floor: 4,
        max_capsules: 8,
        max_tokens: 1024,
        include_supporting: false,
        include_contradictions: false,
        max_hops: 1,
        mode: RetrievalMode::RrfFusionParent,
        embedding: Vec::new(),
    };

    let response = store.query(&request)?;

    assert!(response
        .frame
        .capsules
        .iter()
        .all(|capsule| capsule.capsule_id != 2));
    Ok(())
}

#[test]
fn retrieval_config_hash_changes_with_mode() -> Result<()> {
    let request = QueryRequest {
        query: "backup".to_string(),
        tenant_id: 1,
        as_of: 10,
        context_mask: "0x1".to_string(),
        policy_mask: "0xffff".to_string(),
        trust_floor: 4,
        max_capsules: 8,
        max_tokens: 1024,
        include_supporting: false,
        include_contradictions: false,
        max_hops: 1,
        mode: RetrievalMode::L5m,
        embedding: Vec::new(),
    };
    let mut fused = request.clone();
    fused.mode = RetrievalMode::RrfFusionParent;

    assert_ne!(request.config_hash(), fused.config_hash());
    Ok(())
}
