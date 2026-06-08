// Proves the sublinear LSH candidate generator returns essentially the same
// results as the exact O(N) hamming scan on a diverse corpus — i.e. the speedup
// does not cost accuracy. Pure fingerprint matching (no anchors/entities), so
// this exercises the LSH path directly, not the exact lookup shortcut.

use std::fs;

use l5m_core::{
    compile_segment, retrieve::retrieve_with_config, retrieve::RetrievalConfig, CompileOptions,
    MemoryProbe, Result, Segment,
};
use tempfile::tempdir;

const VOCAB: &[&str] = &[
    "backup",
    "retention",
    "scanning",
    "incident",
    "payment",
    "audit",
    "deploy",
    "warehouse",
    "latency",
    "encryption",
    "rollback",
    "throughput",
    "policy",
    "cluster",
    "billing",
    "schema",
    "token",
    "quota",
    "replica",
    "snapshot",
    "failover",
    "ingest",
    "compaction",
    "tenant",
    "vector",
    "segment",
    "probe",
    "gate",
    "capsule",
    "fingerprint",
    "anchor",
    "evidence",
];

// Deterministic LCG so the corpus is varied but reproducible.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 16
    }
    fn pick<'a>(&mut self, words: &[&'a str]) -> &'a str {
        words[(self.next() as usize) % words.len()]
    }
}

fn varied_text(seed: u64, unique: usize) -> String {
    let mut rng = Lcg(seed);
    let mut words = Vec::new();
    for _ in 0..12 {
        words.push(rng.pick(VOCAB).to_string());
    }
    // a unique token so each capsule has a distinguishable fingerprint
    words.push(format!("zephyr{unique:07}"));
    words.join(" ")
}

fn build_corpus(n: usize) -> Result<(tempfile::TempDir, Segment, Vec<String>)> {
    let dir = tempdir()?;
    let input = dir.path().join("in.json");
    let output = dir.path().join("seg.segment");
    let mut entries = Vec::with_capacity(n);
    let mut texts = Vec::with_capacity(n);
    for i in 0..n {
        let text = varied_text((i as u64).wrapping_mul(2654435761) ^ 0x9e37, i);
        texts.push(text.clone());
        entries.push(format!(
            r#"{{"capsule_id":"{id}","tenant_id":1,"claim":"record {i} {claim}","evidence":"{ev}","source_id":{id},"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,"classification":1,"poison_risk":0}}"#,
            id = i + 1,
            claim = VOCAB[i % VOCAB.len()],
            ev = text,
        ));
    }
    fs::write(&input, format!("[{}]", entries.join(",")))?;
    compile_segment(CompileOptions {
        input_json: input,
        output_segment: output.clone(),
        epoch: 1,
    })?;
    Ok((dir, Segment::open(output)?, texts))
}

fn top_ids(seg: &Segment, query: &str, cfg: &RetrievalConfig) -> Result<Vec<u128>> {
    let mut probe = MemoryProbe::build(query, 1, 1000, 0xffff, 0xffff, 0);
    probe.max_capsules = 10;
    probe.max_tokens = usize::MAX;
    let frame = retrieve_with_config(seg, &probe, cfg)?;
    Ok(frame.capsules.iter().map(|c| c.capsule_id).collect())
}

#[test]
fn ann_agrees_with_exact_scan() -> Result<()> {
    let n = 3000;
    let (_dir, seg, texts) = build_corpus(n)?;

    let exact = RetrievalConfig {
        semantic_hamming_threshold: 180,
        max_scored_candidates: usize::MAX,
        ann_candidate_threshold: usize::MAX, // never use ANN -> full exact scan
        embed_rrf_k: 60.0,
    };
    let ann = RetrievalConfig {
        semantic_hamming_threshold: 180,
        max_scored_candidates: usize::MAX,
        ann_candidate_threshold: 0, // always use the LSH path
        embed_rrf_k: 60.0,
    };

    let queries = 200usize;
    let step = n / queries;
    let mut top1_match = 0usize;
    let mut overlap_sum = 0.0f64;
    for q in 0..queries {
        let target = q * step;
        let query = &texts[target];
        let exact_ids = top_ids(&seg, query, &exact)?;
        let ann_ids = top_ids(&seg, query, &ann)?;
        if exact_ids.first() == ann_ids.first() {
            top1_match += 1;
        }
        let common = ann_ids.iter().filter(|id| exact_ids.contains(id)).count();
        overlap_sum += common as f64 / exact_ids.len().max(1) as f64;
    }
    let top1_rate = top1_match as f64 / queries as f64;
    let mean_overlap = overlap_sum / queries as f64;
    eprintln!("ANN vs exact: top1_rate={top1_rate:.3}, mean_overlap@10={mean_overlap:.3}");

    // The sublinear path must match the exact scan closely.
    assert!(top1_rate >= 0.95, "top-1 agreement too low: {top1_rate:.3}");
    assert!(
        mean_overlap >= 0.85,
        "top-10 overlap too low: {mean_overlap:.3}"
    );
    Ok(())
}
