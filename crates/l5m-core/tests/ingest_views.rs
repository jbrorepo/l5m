use std::fs;

use l5m_core::{compile_product_memories, ProductCompileOptions, Result, Segment};
use tempfile::tempdir;

#[test]
fn product_ingest_writes_multiview_segments_with_parent_metadata() -> Result<()> {
    let dir = tempdir()?;
    let input = dir.path().join("memories.jsonl");
    let out_dir = dir.path().join("segments");
    fs::write(
        &input,
        r#"{"memory_id":"m1","tenant_id":1,"session_id":"s1","observed_at":10,"valid_from":1,"context_mask":"0x1","policy_mask":"0xffff","trust_level":8,"classification":1,"poison_risk":0,"turns":[{"turn_id":"t1","role":"user","text":"I take yoga at Riverbend."},{"turn_id":"t2","role":"assistant","text":"Noted."}]}"#,
    )?;

    let manifest = compile_product_memories(ProductCompileOptions {
        input_jsonl: input,
        output_dir: out_dir,
        epoch: 3,
    })?;

    assert_eq!(manifest.views.len(), 5);
    let user_turn = manifest
        .views
        .iter()
        .find(|view| view.view == "user-turn")
        .expect("user-turn view");
    let segment = Segment::open(&user_turn.segment_path)?;
    let capsule = segment.capsule(0).expect("capsule");
    assert!(capsule
        .source_uri
        .as_deref()
        .unwrap()
        .contains("memory_id=m1"));
    assert!(capsule
        .source_uri
        .as_deref()
        .unwrap()
        .contains("session_id=s1"));
    assert!(capsule
        .source_uri
        .as_deref()
        .unwrap()
        .contains("turn_id=t1"));
    Ok(())
}
