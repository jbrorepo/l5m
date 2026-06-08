use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    fs,
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use l5m_core::{compile_segment, retrieve as l5m_retrieve, CompileOptions, MemoryProbe, Segment};
use serde_json::json;

use crate::{
    adapters::{BenchmarkDocument, BenchmarkItem},
    latency::StageTimings,
    modes::{flat_scan::parent_retrieval_id, token_estimate, ModeRun, ReturnedCapsule},
    runfile::CandidateCounts,
};

pub fn retrieve(item: &BenchmarkItem, top_k: usize) -> Result<ModeRun, Box<dyn Error>> {
    retrieve_documents_with_l5m(item, &item.documents, top_k, None)
}

/// Dense embeddings for one query: the query vector plus per-capsule vectors,
/// keyed by capsule id (as a string). Enables native hybrid retrieval.
pub struct EmbeddingRefs<'a> {
    pub query: &'a [f32],
    pub by_capsule: &'a HashMap<String, Vec<f32>>,
}

pub fn retrieve_with_embeddings(
    item: &BenchmarkItem,
    top_k: usize,
    embeddings: &EmbeddingRefs<'_>,
) -> Result<ModeRun, Box<dyn Error>> {
    retrieve_documents_with_l5m(item, &item.documents, top_k, Some(embeddings))
}

pub(crate) fn retrieve_documents_with_l5m(
    item: &BenchmarkItem,
    documents: &[BenchmarkDocument],
    top_k: usize,
    embeddings: Option<&EmbeddingRefs<'_>>,
) -> Result<ModeRun, Box<dyn Error>> {
    if documents.is_empty() || top_k == 0 {
        return Ok(ModeRun {
            returned: Vec::new(),
            timings: StageTimings::default(),
            candidate_counts: CandidateCounts::default(),
            index_build_time_ns: 0,
            segment_size_bytes: 0,
        });
    }

    let dir = temp_dir()?;
    fs::create_dir_all(&dir)?;
    let input = dir.join("capsules.json");
    let output = dir.join("bench.segment");
    let raw_capsules = documents
        .iter()
        .map(|doc| {
            let mut value = json!({
                "capsule_id": doc.capsule_id.to_string(),
                "tenant_id": 1,
                "claim": first_line_or_text(&doc.text),
                "evidence": doc.text,
                "source_id": source_id(doc.capsule_id),
                "source_uri": format!(
                    "bench://{}/{}/{}/{}/{}",
                    doc.parent.benchmark_name,
                    doc.parent.benchmark_query_id,
                    doc.parent.parent_session_id.as_deref().unwrap_or("-"),
                    doc.parent.parent_dialog_id.as_deref().unwrap_or("-"),
                    doc.parent.parent_evidence_id.as_deref().unwrap_or("-")
                ),
                "valid_from": 1,
                "observed_at": 1,
                "last_verified_at": 1,
                "context_mask": "0xffff",
                "policy_mask": "0xffff",
                "trust_level": 8,
                "classification": 1,
                "poison_risk": 0
            });
            if let Some(embeddings) = embeddings {
                if let Some(vector) = embeddings.by_capsule.get(&doc.capsule_id.to_string()) {
                    value["embedding"] = json!(vector);
                }
            }
            value
        })
        .collect::<Vec<_>>();
    let build_start = Instant::now();
    fs::write(&input, serde_json::to_string(&raw_capsules)?)?;
    compile_segment(CompileOptions {
        input_json: input,
        output_segment: output.clone(),
        epoch: 1,
    })?;
    let segment = Segment::open(&output)?;
    let build_or_load_ns = build_start.elapsed().as_nanos() as u64;
    let segment_size_bytes = fs::metadata(&output)?.len();

    let probe_start = Instant::now();
    let mut probe = MemoryProbe::build(&item.question, 1, 1, 0xffff, 0xffff, 0);
    probe.max_capsules = top_k;
    probe.max_tokens = usize::MAX;
    if let Some(embeddings) = embeddings {
        probe.embedding = embeddings.query.to_vec();
    }
    let probe_build_ns = probe_start.elapsed().as_nanos() as u64;

    let retrieval_start = Instant::now();
    let frame = l5m_retrieve(&segment, &probe)?;
    let total_retrieval_ns = retrieval_start.elapsed().as_nanos() as u64;
    let candidate_count_before_scoring = frame.coverage.candidate_count_before_scoring;
    let parent_by_capsule = documents
        .iter()
        .map(|doc| (doc.capsule_id, parent_retrieval_id(item, doc)))
        .collect::<BTreeMap<_, _>>();
    let text_by_capsule = documents
        .iter()
        .map(|doc| (doc.capsule_id, doc.text.as_str()))
        .collect::<BTreeMap<_, _>>();

    let returned = frame
        .capsules
        .into_iter()
        .filter_map(|capsule| {
            parent_by_capsule
                .get(&capsule.capsule_id)
                .map(|parent_id| ReturnedCapsule {
                    capsule_id: capsule.capsule_id.to_string(),
                    parent_id: parent_id.clone(),
                    token_estimate: text_by_capsule
                        .get(&capsule.capsule_id)
                        .map_or(1, |text| token_estimate(text)),
                })
        })
        .collect();

    Ok(ModeRun {
        returned,
        timings: StageTimings {
            build_or_load_ns,
            probe_build_ns,
            semantic_score_ns: total_retrieval_ns,
            total_retrieval_ns,
            ..StageTimings::default()
        },
        candidate_counts: CandidateCounts {
            candidate_count_initial: documents.len(),
            candidate_count_after_hard_gates: candidate_count_before_scoring,
            candidate_count_before_scoring,
            candidate_count_scored: candidate_count_before_scoring,
        },
        index_build_time_ns: build_or_load_ns,
        segment_size_bytes,
    })
}

fn temp_dir() -> Result<PathBuf, Box<dyn Error>> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    Ok(std::env::temp_dir().join(format!("l5m-benchmarks-{nanos}")))
}

fn first_line_or_text(text: &str) -> &str {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(text)
}

fn source_id(capsule_id: u128) -> u64 {
    u64::try_from(capsule_id).unwrap_or_else(|_| {
        let low = capsule_id as u64;
        let high = (capsule_id >> 64) as u64;
        low ^ high
    })
}
