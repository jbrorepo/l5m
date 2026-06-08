use serde::{Deserialize, Serialize};

use crate::{latency::StageTimings, metrics::ScoreSet};

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct CandidateCounts {
    pub candidate_count_initial: usize,
    pub candidate_count_after_hard_gates: usize,
    pub candidate_count_before_scoring: usize,
    pub candidate_count_scored: usize,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct GateViolations {
    pub tenant_violation: bool,
    pub context_violation: bool,
    pub policy_violation: bool,
    pub trust_violation: bool,
    pub temporal_violation: bool,
    pub poison_violation: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RunRow {
    pub benchmark: String,
    pub query_id: String,
    pub question: String,
    pub mode: String,
    pub top_k: usize,
    pub category: String,
    pub ground_truth_ids: Vec<String>,
    pub returned_parent_ids: Vec<String>,
    pub returned_capsule_ids: Vec<String>,
    pub scores: ScoreSet,
    pub timings: StageTimings,
    pub candidate_counts: CandidateCounts,
    pub gate_violations: GateViolations,
    pub returned_capsule_count: usize,
    pub returned_token_estimate: usize,
    pub index_build_time_ns: u64,
    pub segment_size_bytes: u64,
    #[serde(default)]
    pub missed_ground_truth_ids: Vec<String>,
    #[serde(default)]
    pub hit_ranks: Vec<usize>,
    #[serde(default)]
    pub retrieval_granularity: String,
    #[serde(default)]
    pub candidate_pool_exhausted: bool,
    #[serde(default)]
    pub retrieves_all_mode: bool,
    #[serde(default = "default_true")]
    pub raw_retrieval_only: bool,
    #[serde(default)]
    pub config_hash: String,
    #[serde(default)]
    pub dataset_hash: String,
    #[serde(default)]
    pub split_hash: String,
}

fn default_true() -> bool {
    true
}

impl RunRow {
    #[cfg(test)]
    pub fn minimal(
        benchmark: &str,
        query_id: &str,
        question: &str,
        mode: &str,
        top_k: usize,
    ) -> Self {
        Self {
            benchmark: benchmark.to_string(),
            query_id: query_id.to_string(),
            question: question.to_string(),
            mode: mode.to_string(),
            top_k,
            category: "unknown".to_string(),
            ground_truth_ids: Vec::new(),
            returned_parent_ids: Vec::new(),
            returned_capsule_ids: Vec::new(),
            scores: ScoreSet::default(),
            timings: StageTimings::default(),
            candidate_counts: CandidateCounts::default(),
            gate_violations: GateViolations::default(),
            returned_capsule_count: 0,
            returned_token_estimate: 0,
            index_build_time_ns: 0,
            segment_size_bytes: 0,
            missed_ground_truth_ids: Vec::new(),
            hit_ranks: Vec::new(),
            retrieval_granularity: "session".to_string(),
            candidate_pool_exhausted: false,
            retrieves_all_mode: false,
            raw_retrieval_only: true,
            config_hash: String::new(),
            dataset_hash: String::new(),
            split_hash: String::new(),
        }
    }
}

pub fn encode_jsonl(rows: &[RunRow]) -> serde_json::Result<String> {
    let mut out = String::new();
    for row in rows {
        out.push_str(&serde_json::to_string(row)?);
        out.push('\n');
    }
    Ok(out)
}

pub fn decode_jsonl(input: &str) -> serde_json::Result<Vec<RunRow>> {
    input
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runfile_jsonl_round_trips() {
        let row = RunRow::minimal("LongMemEval", "q1", "Where?", "bm25", 10);
        let encoded = encode_jsonl(std::slice::from_ref(&row)).unwrap();
        let decoded = decode_jsonl(&encoded).unwrap();

        assert_eq!(decoded, vec![row]);
    }

    #[test]
    fn gate_violation_fields_default_false_for_safe_runs() {
        let row = RunRow::minimal("LongMemEval", "q1", "Where?", "bm25", 10);

        assert!(!row.gate_violations.tenant_violation);
        assert!(!row.gate_violations.context_violation);
        assert!(!row.gate_violations.policy_violation);
        assert!(!row.gate_violations.trust_violation);
        assert!(!row.gate_violations.temporal_violation);
        assert!(!row.gate_violations.poison_violation);
    }

    #[test]
    fn runfile_round_trip_preserves_audit_fields() {
        let mut row = RunRow::minimal("LongMemEval", "q1", "Where?", "hybrid-parent", 10);
        row.missed_ground_truth_ids = vec!["s2".to_string()];
        row.hit_ranks = vec![1, 4];
        row.retrieval_granularity = "session".to_string();
        row.candidate_pool_exhausted = false;
        row.retrieves_all_mode = false;
        row.raw_retrieval_only = true;
        row.config_hash = "cfg".to_string();
        row.dataset_hash = "data".to_string();
        row.split_hash = "split".to_string();

        let encoded = encode_jsonl(std::slice::from_ref(&row)).unwrap();
        let decoded = decode_jsonl(&encoded).unwrap();

        assert_eq!(decoded, vec![row]);
    }
}
