// E6: gate-filtered dense ANN. With the sublinear ANN candidate path engaged, a
// capsule that is cosine-near the query but shares NO lexical/fingerprint signal
// must still be surfaced via the embedding LSH — and an unauthorized cosine match
// must still be gated out.

use std::fs;

use l5m_core::retrieve::{retrieve_with_config, RetrievalConfig};
use l5m_core::{compile_segment, CompileOptions, MemoryProbe, Result, Segment};
use tempfile::tempdir;

fn emb8(pos: usize) -> String {
    let mut v = [0.0f32; 8];
    v[pos] = 1.0;
    let parts: Vec<String> = v.iter().map(|x| x.to_string()).collect();
    format!("[{}]", parts.join(","))
}

fn corpus() -> String {
    let mut entries = Vec::new();
    // 10 fillers: share the query's lexical/fingerprint signal, embeddings spread
    // across dims 1..=7 (orthogonal to the target at dim 0).
    for i in 0..10 {
        entries.push(format!(
            r#"{{"capsule_id":"{id}","tenant_id":1,"claim":"alpha beta gamma delta filler {i}","evidence":"alpha beta gamma delta filler {i}","source_id":{id},"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,"classification":1,"poison_risk":0,"embedding":{emb}}}"#,
            id = i + 1,
            emb = emb8((i % 7) + 1),
        ));
    }
    // Target (id 100): lexically DISJOINT from the query, but its embedding == the
    // query embedding (dim 0).
    entries.push(format!(
        r#"{{"capsule_id":"100","tenant_id":1,"claim":"needle qux zlorp","evidence":"needle qux zlorp","source_id":100,"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,"classification":1,"poison_risk":0,"embedding":{}}}"#,
        emb8(0)
    ));
    // Unauthorized (tenant 2) capsule with the SAME embedding as the target.
    entries.push(format!(
        r#"{{"capsule_id":"200","tenant_id":2,"claim":"needle qux zlorp secret","evidence":"needle qux zlorp secret","source_id":200,"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,"classification":1,"poison_risk":0,"embedding":{}}}"#,
        emb8(0)
    ));
    format!("[{}]", entries.join(","))
}

fn segment() -> Result<(tempfile::TempDir, Segment)> {
    let dir = tempdir()?;
    let input = dir.path().join("in.json");
    let out = dir.path().join("seg.segment");
    fs::write(&input, corpus())?;
    compile_segment(CompileOptions {
        input_json: input,
        output_segment: out.clone(),
        epoch: 1,
    })?;
    Ok((dir, Segment::open(out)?))
}

// Force the sublinear ANN path with a tight scoring cap.
fn ann_cfg() -> RetrievalConfig {
    RetrievalConfig {
        semantic_hamming_threshold: 180,
        max_scored_candidates: 5,
        ann_candidate_threshold: 0,
        embed_rrf_k: 60.0,
    }
}

fn probe(query: &str) -> MemoryProbe {
    let mut p = MemoryProbe::build(query, 1, 1000, 0xffff, 0xffff, 0);
    p.max_capsules = 8;
    p
}

#[test]
fn dense_ann_surfaces_a_lexically_disjoint_match() -> Result<()> {
    let (_d, seg) = segment()?;

    // Without a query embedding: the ANN pool is fingerprint/lexical-only, so the
    // lexically-disjoint target never enters the scored set.
    let lexical = retrieve_with_config(&seg, &probe("alpha beta gamma delta"), &ann_cfg())?;
    assert!(
        lexical.capsules.iter().all(|c| c.capsule_id != 100),
        "without embeddings the dense-only target should be missed under the ANN cap"
    );

    // With the query embedding: the embedding LSH surfaces the cosine-near target.
    let mut p = probe("alpha beta gamma delta");
    p.embedding = vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let hybrid = retrieve_with_config(&seg, &p, &ann_cfg())?;
    assert!(
        hybrid.capsules.iter().any(|c| c.capsule_id == 100),
        "dense ANN should surface the cosine-near, lexically-disjoint capsule"
    );
    Ok(())
}

#[test]
fn dense_ann_candidates_are_still_gated() -> Result<()> {
    let (_d, seg) = segment()?;
    let mut p = probe("alpha beta gamma delta");
    p.embedding = vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let frame = retrieve_with_config(&seg, &p, &ann_cfg())?;
    assert!(
        frame.capsules.iter().all(|c| c.capsule_id != 200),
        "unauthorized (tenant-2) cosine match must be gated out of the dense ANN pool"
    );
    Ok(())
}
