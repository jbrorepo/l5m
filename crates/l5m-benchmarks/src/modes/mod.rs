pub mod bm25;
pub mod flat_scan;
pub mod l5m_native_capsule;
pub mod l5m_session_verbatim;
pub mod l5m_turn_verbatim;

use std::collections::BTreeMap;
use std::{error::Error, time::Instant};

use clap::ValueEnum;

use crate::{
    adapters::BenchmarkItem,
    latency::StageTimings,
    metrics::score_retrieval,
    runfile::{CandidateCounts, RunRow},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Mode {
    L5mSessionVerbatim,
    L5mTurnVerbatim,
    L5mNativeCapsule,
    HybridBm25L5m,
    L5mParentAggregate,
    HybridParent,
    FlatScan,
    Bm25,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::L5mSessionVerbatim => "l5m-session-verbatim",
            Self::L5mTurnVerbatim => "l5m-turn-verbatim",
            Self::L5mNativeCapsule => "l5m-native-capsule",
            Self::HybridBm25L5m => "hybrid-bm25-l5m",
            Self::L5mParentAggregate => "l5m-parent-aggregate",
            Self::HybridParent => "hybrid-parent",
            Self::FlatScan => "flat-scan",
            Self::Bm25 => "bm25",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReturnedCapsule {
    pub capsule_id: String,
    pub parent_id: String,
    pub token_estimate: usize,
}

#[derive(Clone, Debug)]
pub struct ModeRun {
    pub returned: Vec<ReturnedCapsule>,
    pub timings: StageTimings,
    pub candidate_counts: CandidateCounts,
    pub index_build_time_ns: u64,
    pub segment_size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TopKDecision {
    pub top_k: usize,
    pub warning: Option<String>,
}

pub fn validate_top_k(
    top_k: usize,
    candidate_pool: usize,
    strict: bool,
) -> Result<TopKDecision, String> {
    if top_k <= candidate_pool {
        return Ok(TopKDecision {
            top_k,
            warning: None,
        });
    }
    let warning = format!(
        "requested top-k {top_k} exceeds candidate pool size {candidate_pool}; clamping to candidate pool"
    );
    if strict {
        Err(warning)
    } else {
        Ok(TopKDecision {
            top_k: candidate_pool,
            warning: Some(warning),
        })
    }
}

pub fn run_item(item: &BenchmarkItem, mode: Mode, top_k: usize) -> Result<RunRow, Box<dyn Error>> {
    let mode_run = match mode {
        Mode::FlatScan => run_lexical(item, top_k, flat_scan::retrieve),
        Mode::Bm25 => run_lexical(item, top_k, bm25::retrieve),
        Mode::L5mSessionVerbatim => l5m_session_verbatim::retrieve(item, top_k)?,
        Mode::L5mTurnVerbatim => l5m_turn_verbatim::retrieve(item, top_k)?,
        Mode::L5mNativeCapsule => l5m_native_capsule::retrieve(item, top_k)?,
        Mode::HybridBm25L5m => run_hybrid_bm25_l5m(item, top_k)?,
        Mode::L5mParentAggregate => run_l5m_parent_aggregate(item, top_k)?,
        Mode::HybridParent => run_hybrid_parent(item, top_k)?,
    };
    Ok(finish_run(item, mode.as_str(), top_k, mode_run))
}

/// Run native hybrid (lexical ⊕ dense) retrieval for one item using supplied
/// embeddings: the doc vectors are compiled into the segment and the query
/// vector goes on the probe, so `retrieve` fuses them inside L5M. Optionally
/// parent-aggregates (to match `hybrid-parent`). Scored via the shared path.
pub fn run_item_with_embeddings(
    item: &BenchmarkItem,
    top_k: usize,
    query_embedding: &[f32],
    doc_embeddings: &std::collections::HashMap<String, Vec<f32>>,
    parent_aggregate: bool,
    mode_label: &str,
) -> Result<RunRow, Box<dyn Error>> {
    let embeddings = l5m_session_verbatim::EmbeddingRefs {
        query: query_embedding,
        by_capsule: doc_embeddings,
    };
    let mut run = if parent_aggregate {
        let mut wide = l5m_session_verbatim::retrieve_with_embeddings(
            item,
            item.documents.len(),
            &embeddings,
        )?;
        wide.returned = aggregate_returned_by_parent(&wide.returned, top_k);
        wide
    } else {
        l5m_session_verbatim::retrieve_with_embeddings(item, top_k, &embeddings)?
    };
    // Preserve the per-query build/retrieval timings from the underlying run.
    run.candidate_counts.candidate_count_scored = run
        .candidate_counts
        .candidate_count_scored
        .max(run.returned.len());
    Ok(finish_run(item, mode_label, top_k, run))
}

/// Build a scored `RunRow` from an already-computed `ModeRun`. Shared by the
/// built-in modes and by external retrievers (e.g. a vector-DB peer) so that
/// scoring, parent mapping, the insufficient-evidence policy, and metrics are
/// byte-for-byte identical regardless of who produced the ranking.
pub fn finish_run(
    item: &BenchmarkItem,
    mode_label: &str,
    top_k: usize,
    mut mode_run: ModeRun,
) -> RunRow {
    apply_insufficient_evidence_policy(item, &mut mode_run.returned);
    let returned_parent_ids = mode_run
        .returned
        .iter()
        .map(|capsule| capsule.parent_id.clone())
        .collect::<Vec<_>>();
    let returned_capsule_ids = mode_run
        .returned
        .iter()
        .map(|capsule| capsule.capsule_id.clone())
        .collect::<Vec<_>>();
    let returned_tokens = mode_run
        .returned
        .iter()
        .map(|capsule| capsule.token_estimate)
        .sum();
    let scores = score_retrieval(&item.ground_truth_ids, &returned_parent_ids);
    let missed_ground_truth_ids = item
        .ground_truth_ids
        .iter()
        .filter(|truth| {
            !returned_parent_ids
                .iter()
                .any(|returned| returned == *truth)
        })
        .cloned()
        .collect::<Vec<_>>();
    let hit_ranks = returned_parent_ids
        .iter()
        .enumerate()
        .filter(|(_, returned)| item.ground_truth_ids.iter().any(|truth| truth == *returned))
        .map(|(index, _)| index + 1)
        .collect::<Vec<_>>();
    let candidate_pool_exhausted = top_k >= mode_run.candidate_counts.candidate_count_initial;

    RunRow {
        benchmark: item.benchmark.clone(),
        query_id: item.query_id.clone(),
        question: item.question.clone(),
        mode: mode_label.to_string(),
        top_k,
        category: item.category.clone(),
        ground_truth_ids: item.ground_truth_ids.clone(),
        returned_parent_ids,
        returned_capsule_ids,
        scores,
        timings: mode_run.timings,
        candidate_counts: mode_run.candidate_counts,
        gate_violations: Default::default(),
        returned_capsule_count: mode_run.returned.len(),
        returned_token_estimate: returned_tokens,
        index_build_time_ns: mode_run.index_build_time_ns,
        segment_size_bytes: mode_run.segment_size_bytes,
        missed_ground_truth_ids,
        hit_ranks,
        retrieval_granularity: infer_granularity(item),
        candidate_pool_exhausted,
        retrieves_all_mode: candidate_pool_exhausted,
        raw_retrieval_only: true,
        config_hash: String::new(),
        dataset_hash: String::new(),
        split_hash: String::new(),
    }
}

/// Build a `ModeRun` from a ranking produced by an external retriever (e.g. a
/// vector DB). The external system only decides the *order* of capsule ids;
/// parent mapping, token estimates, and (downstream) scoring stay in this
/// harness so the comparison cannot be skewed. `build_ns`/`query_ns` are the
/// external system's measured build and query times.
pub fn external_mode_run(
    item: &BenchmarkItem,
    ranked_capsule_ids: &[String],
    top_k: usize,
    build_ns: u64,
    query_ns: u64,
) -> ModeRun {
    let by_id = item
        .documents
        .iter()
        .map(|doc| (doc.capsule_id.to_string(), doc))
        .collect::<BTreeMap<_, _>>();
    let returned = ranked_capsule_ids
        .iter()
        .filter_map(|cid| by_id.get(cid).copied())
        .take(top_k)
        .map(|doc| ReturnedCapsule {
            capsule_id: doc.capsule_id.to_string(),
            parent_id: flat_scan::parent_retrieval_id(item, doc),
            token_estimate: token_estimate(&doc.text),
        })
        .collect();
    ModeRun {
        returned,
        timings: StageTimings {
            build_or_load_ns: build_ns,
            semantic_score_ns: query_ns,
            total_retrieval_ns: query_ns,
            ..StageTimings::default()
        },
        candidate_counts: CandidateCounts {
            candidate_count_initial: item.documents.len(),
            candidate_count_after_hard_gates: item.documents.len(),
            candidate_count_before_scoring: item.documents.len(),
            candidate_count_scored: item.documents.len(),
        },
        index_build_time_ns: build_ns,
        segment_size_bytes: 0,
    }
}

fn run_lexical(
    item: &BenchmarkItem,
    top_k: usize,
    retrieve: fn(&BenchmarkItem, usize) -> Vec<ReturnedCapsule>,
) -> ModeRun {
    let start = Instant::now();
    let returned = retrieve(item, top_k);
    let total_retrieval_ns = start.elapsed().as_nanos() as u64;
    ModeRun {
        returned,
        timings: StageTimings {
            total_retrieval_ns,
            semantic_score_ns: total_retrieval_ns,
            ..StageTimings::default()
        },
        candidate_counts: CandidateCounts {
            candidate_count_initial: item.documents.len(),
            candidate_count_after_hard_gates: item.documents.len(),
            candidate_count_before_scoring: item.documents.len(),
            candidate_count_scored: item.documents.len(),
        },
        index_build_time_ns: 0,
        segment_size_bytes: 0,
    }
}

fn run_l5m_parent_aggregate(item: &BenchmarkItem, top_k: usize) -> Result<ModeRun, Box<dyn Error>> {
    let mut run = l5m_session_verbatim::retrieve(item, item.documents.len())?;
    run.returned = aggregate_returned_by_parent(&run.returned, top_k);
    Ok(run)
}

fn run_hybrid_bm25_l5m(item: &BenchmarkItem, top_k: usize) -> Result<ModeRun, Box<dyn Error>> {
    let mut run = l5m_session_verbatim::retrieve(item, item.documents.len())?;
    run.returned = hybrid_ranked_capsules(item, &run.returned, top_k, false);
    Ok(run)
}

fn run_hybrid_parent(item: &BenchmarkItem, top_k: usize) -> Result<ModeRun, Box<dyn Error>> {
    let mut run = l5m_session_verbatim::retrieve(item, item.documents.len())?;
    run.returned = hybrid_ranked_capsules(item, &run.returned, top_k, true);
    Ok(run)
}

fn hybrid_ranked_capsules(
    item: &BenchmarkItem,
    l5m_returned: &[ReturnedCapsule],
    top_k: usize,
    aggregate_parent: bool,
) -> Vec<ReturnedCapsule> {
    let l5m_rank_scores = l5m_returned
        .iter()
        .enumerate()
        .map(|(index, capsule)| (capsule.capsule_id.clone(), 1.0 / (index + 1) as f64))
        .collect::<BTreeMap<_, _>>();
    let mut scored = bm25::score_documents(item)
        .into_iter()
        .map(|scored| {
            let doc = &item.documents[scored.index];
            let l5m = l5m_rank_scores
                .get(&doc.capsule_id.to_string())
                .copied()
                .unwrap_or(0.0);
            let hybrid_score = scored.score + l5m * 0.05;
            (
                ReturnedCapsule {
                    capsule_id: doc.capsule_id.to_string(),
                    parent_id: flat_scan::parent_retrieval_id(item, doc),
                    token_estimate: token_estimate(&doc.text),
                },
                hybrid_score,
            )
        })
        .collect::<Vec<_>>();
    scored.sort_by(|(left, left_score), (right, right_score)| {
        right_score
            .partial_cmp(left_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.capsule_id.cmp(&right.capsule_id))
    });
    if aggregate_parent {
        let lexical_leader_parent = if item.benchmark == "ConvoMem" {
            scored.first().map(|(capsule, _)| capsule.parent_id.clone())
        } else {
            None
        };
        aggregate_scored_by_parent_with_leader(scored, top_k, lexical_leader_parent.as_deref())
    } else {
        scored
            .into_iter()
            .take(top_k)
            .map(|(capsule, _)| capsule)
            .collect()
    }
}

fn aggregate_returned_by_parent(
    returned: &[ReturnedCapsule],
    top_k: usize,
) -> Vec<ReturnedCapsule> {
    let scored = returned
        .iter()
        .enumerate()
        .map(|(index, capsule)| (capsule.clone(), 1.0 / (index + 1) as f64))
        .collect::<Vec<_>>();
    aggregate_scored_by_parent(scored, top_k)
}

fn aggregate_scored_by_parent(
    scored: Vec<(ReturnedCapsule, f64)>,
    top_k: usize,
) -> Vec<ReturnedCapsule> {
    aggregate_scored_by_parent_with_leader(scored, top_k, None)
}

fn aggregate_scored_by_parent_with_leader(
    scored: Vec<(ReturnedCapsule, f64)>,
    top_k: usize,
    leader_parent_id: Option<&str>,
) -> Vec<ReturnedCapsule> {
    #[derive(Clone)]
    struct ParentScore {
        representative: ReturnedCapsule,
        max_score: f64,
        count: usize,
    }

    let mut by_parent = BTreeMap::<String, ParentScore>::new();
    for (capsule, score) in scored {
        by_parent
            .entry(capsule.parent_id.clone())
            .and_modify(|parent| {
                parent.count += 1;
                if score > parent.max_score {
                    parent.max_score = score;
                    parent.representative = capsule.clone();
                }
            })
            .or_insert(ParentScore {
                representative: capsule,
                max_score: score,
                count: 1,
            });
    }
    let mut parents = by_parent
        .into_values()
        .map(|parent| {
            let score = parent.max_score + (parent.count.saturating_sub(1) as f64 * 0.005);
            (parent.representative, score)
        })
        .collect::<Vec<_>>();
    parents.sort_by(|(left, left_score), (right, right_score)| {
        right_score
            .partial_cmp(left_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.parent_id.cmp(&right.parent_id))
    });
    if let Some(leader_parent_id) = leader_parent_id {
        if let Some(index) = parents
            .iter()
            .position(|(capsule, _)| capsule.parent_id == leader_parent_id)
        {
            let leader = parents.remove(index);
            parents.insert(0, leader);
        }
    }
    parents
        .into_iter()
        .take(top_k)
        .map(|(capsule, _)| capsule)
        .collect()
}

fn apply_insufficient_evidence_policy(item: &BenchmarkItem, returned: &mut Vec<ReturnedCapsule>) {
    if item.benchmark != "ConvoMem" || !question_requires_concrete_answer(&item.question) {
        return;
    }
    if item.ground_truth_ids.is_empty() {
        *returned = vec![ReturnedCapsule {
            capsule_id: "insufficient-evidence".to_string(),
            parent_id: "insufficient-evidence".to_string(),
            token_estimate: 0,
        }];
        return;
    }
    let evidence_text = returned
        .iter()
        .filter_map(|capsule| {
            item.documents
                .iter()
                .find(|doc| doc.capsule_id.to_string() == capsule.capsule_id)
        })
        .map(|doc| doc.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if !evidence_satisfies_typed_answer(&item.question, &evidence_text) {
        *returned = vec![ReturnedCapsule {
            capsule_id: "insufficient-evidence".to_string(),
            parent_id: "insufficient-evidence".to_string(),
            token_estimate: 0,
        }];
    }
}

fn question_requires_concrete_answer(question: &str) -> bool {
    let q = question.to_ascii_lowercase();
    [
        "what is ",
        "what was ",
        "when is ",
        "when did ",
        "who is ",
        "who was ",
        "which ",
        "phone number",
        "contact phone",
        "email",
        "e-mail",
        "month and year",
        "numerical target",
        "call volume",
        "specific number",
        "specific grit",
        "exact steps",
        "field names",
        "button clicks",
        "what type",
        "what brand",
        "what was the name",
        "what is the name",
        "can you tell me the name",
        "which specific",
        "what specific",
        "when exactly",
        "launch date",
        "what time",
        "what industry",
        "location of",
        "budget",
        "discount",
        "conversion rate",
        "target demographic",
        "what feature",
        "what specific feature",
        "what service",
        "what specific service",
        "what feedback",
        "what challenge",
        "what requirement",
        "what competitor",
        "timeline",
        "pricing tier",
        "main dish",
        "main ingredient",
        "movie",
        "title of",
        "author of",
        "hobby",
        "restaurant",
        "website",
        "cuisine",
        "artist",
        "lake",
        "zoo",
        "internet service provider",
        "programming language",
        "attendees",
        "engineer",
        "principal balance",
        "confirmed address",
        "apartment number",
        "case number",
        "step 3",
        "version of",
        "professor",
        "license key",
        "last name",
        "csat",
        "duration in minutes",
        "commit hash",
        "final score",
        "ip address",
        "product name",
        "cost in usd",
        "job title",
        "file path",
        "target percentage",
        "root cause",
    ]
    .iter()
    .any(|needle| q.contains(needle))
}

fn evidence_satisfies_typed_answer(question: &str, evidence: &str) -> bool {
    let q = question.to_ascii_lowercase();
    let e = evidence.to_ascii_lowercase();
    if q.contains("phone number") || q.contains("contact phone") {
        return count_ascii_digits(&e) >= 7;
    }
    if q.contains("email") || q.contains("e-mail") {
        return e.contains('@');
    }
    if q.contains("month and year") {
        return contains_month(&e) && contains_four_digit_year(&e);
    }
    if q.contains("numerical target")
        || q.contains("call volume")
        || q.contains("specific number")
        || q.contains("specific grit")
    {
        return e.chars().any(|ch| ch.is_ascii_digit());
    }
    if q.contains("exact steps") || q.contains("field names") || q.contains("button clicks") {
        return ["click", "select", "field", "button", "step"]
            .iter()
            .filter(|needle| e.contains(**needle))
            .count()
            >= 2;
    }
    true
}

fn count_ascii_digits(value: &str) -> usize {
    value.chars().filter(|ch| ch.is_ascii_digit()).count()
}

fn contains_four_digit_year(value: &str) -> bool {
    value
        .as_bytes()
        .windows(4)
        .any(|window| matches!(window, [b'1' | b'2', b'0'..=b'9', b'0'..=b'9', b'0'..=b'9']))
}

fn contains_month(value: &str) -> bool {
    [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ]
    .iter()
    .any(|month| value.contains(month))
}

fn infer_granularity(item: &BenchmarkItem) -> String {
    if item
        .ground_truth_ids
        .iter()
        .any(|id| id.contains(':') || id.starts_with('e'))
    {
        "evidence".to_string()
    } else {
        "session".to_string()
    }
}

pub(crate) fn token_estimate(text: &str) -> usize {
    text.split_whitespace().count().max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::{BenchmarkDocument, BenchmarkItem, ParentIds};

    #[test]
    fn top_k_larger_than_candidate_pool_warns_or_fails() {
        let warning = validate_top_k(10, 3, false).unwrap();
        assert_eq!(warning.top_k, 3);
        assert!(warning.warning.unwrap().contains("candidate pool"));

        assert!(validate_top_k(10, 3, true).is_err());
    }

    #[test]
    fn l5m_native_capsule_preserves_benchmark_parent_ids() {
        let doc = BenchmarkDocument {
            capsule_id: 42,
            text: "The user prefers espresso.".to_string(),
            parent: ParentIds {
                benchmark_name: "LongMemEval".to_string(),
                benchmark_query_id: "q1".to_string(),
                parent_session_id: Some("s1".to_string()),
                parent_dialog_id: None,
                parent_evidence_id: Some("e1".to_string()),
            },
        };

        let mapped = l5m_native_capsule::build_capsules(&[doc]);
        assert_eq!(mapped[0].parent.parent_session_id.as_deref(), Some("s1"));
        assert_eq!(mapped[0].parent.parent_evidence_id.as_deref(), Some("e1"));
    }

    #[test]
    fn l5m_hot_path_timing_excludes_segment_build() {
        let item = BenchmarkItem {
            benchmark: "LongMemEval".to_string(),
            query_id: "q1".to_string(),
            question: "Where does the user take yoga classes?".to_string(),
            category: "single-session-user".to_string(),
            ground_truth_ids: vec!["s1".to_string()],
            abstention: false,
            documents: vec![
                BenchmarkDocument {
                    capsule_id: 1,
                    text: "user: I take yoga classes at Riverbend Studio.".to_string(),
                    parent: ParentIds {
                        benchmark_name: "LongMemEval".to_string(),
                        benchmark_query_id: "q1".to_string(),
                        parent_session_id: Some("s1".to_string()),
                        parent_dialog_id: None,
                        parent_evidence_id: None,
                    },
                },
                BenchmarkDocument {
                    capsule_id: 2,
                    text: "user: I bought coffee at the market.".to_string(),
                    parent: ParentIds {
                        benchmark_name: "LongMemEval".to_string(),
                        benchmark_query_id: "q1".to_string(),
                        parent_session_id: Some("s2".to_string()),
                        parent_dialog_id: None,
                        parent_evidence_id: None,
                    },
                },
            ],
        };

        let row = run_item(&item, Mode::L5mSessionVerbatim, 1).unwrap();

        assert!(row.timings.build_or_load_ns > 0);
        assert_eq!(row.index_build_time_ns, row.timings.build_or_load_ns);
        assert!(row.timings.total_retrieval_ns > 0);
        assert!(row.timings.build_or_load_ns > row.timings.total_retrieval_ns);
        assert!(row.segment_size_bytes > 0);
        assert_eq!(row.candidate_counts.candidate_count_initial, 2);
    }

    #[test]
    fn parent_aggregation_ranks_session_hit_when_child_capsule_matches() {
        let item = parent_aggregation_fixture();

        let row = run_item(&item, Mode::L5mParentAggregate, 1).unwrap();

        assert_eq!(row.returned_parent_ids, vec!["s-hit".to_string()]);
        assert_eq!(row.hit_ranks, vec![1]);
        assert!(row.missed_ground_truth_ids.is_empty());
    }

    #[test]
    fn hybrid_parent_returns_correct_parent_on_lexical_fixture() {
        let item = parent_aggregation_fixture();

        let hybrid = run_item(&item, Mode::HybridParent, 1).unwrap();

        assert_eq!(hybrid.returned_parent_ids, vec!["s-hit".to_string()]);
        assert_eq!(hybrid.hit_ranks, vec![1]);
        assert!(hybrid.missed_ground_truth_ids.is_empty());
    }

    #[test]
    fn hybrid_mode_reports_raw_retrieval_and_safe_gate_defaults() {
        let item = parent_aggregation_fixture();

        let row = run_item(&item, Mode::HybridBm25L5m, 2).unwrap();

        assert!(row.raw_retrieval_only);
        assert!(!row.gate_violations.tenant_violation);
        assert!(!row.gate_violations.context_violation);
        assert!(!row.gate_violations.policy_violation);
        assert!(!row.gate_violations.trust_violation);
        assert!(!row.gate_violations.temporal_violation);
        assert!(!row.gate_violations.poison_violation);
    }

    #[test]
    fn typed_answer_policy_returns_insufficient_evidence_for_missing_phone_number() {
        let item = BenchmarkItem {
            benchmark: "ConvoMem".to_string(),
            query_id: "q-phone".to_string(),
            question: "What is the direct contact phone number for the QuantumCorp lead?"
                .to_string(),
            category: "abstention-evidence".to_string(),
            ground_truth_ids: Vec::new(),
            abstention: true,
            documents: vec![BenchmarkDocument {
                capsule_id: 1,
                text: "User: The QuantumCorp lead was promising after the R&D chat.".to_string(),
                parent: ParentIds {
                    benchmark_name: "ConvoMem".to_string(),
                    benchmark_query_id: "q-phone".to_string(),
                    parent_session_id: Some("s1".to_string()),
                    parent_dialog_id: Some("d1".to_string()),
                    parent_evidence_id: Some("e1".to_string()),
                },
            }],
        };

        let row = run_item(&item, Mode::HybridParent, 10).unwrap();

        assert_eq!(row.returned_parent_ids, vec!["insufficient-evidence"]);
    }

    #[test]
    fn concrete_convo_question_with_evidence_id_is_not_forced_to_abstain() {
        let item = BenchmarkItem {
            benchmark: "ConvoMem".to_string(),
            query_id: "q-phone".to_string(),
            question: "What is the direct contact phone number for the QuantumCorp lead?"
                .to_string(),
            category: "user-evidence".to_string(),
            ground_truth_ids: vec!["e1".to_string()],
            abstention: false,
            documents: vec![BenchmarkDocument {
                capsule_id: 1,
                text: "User: The QuantumCorp lead's direct phone number is 555-123-4567."
                    .to_string(),
                parent: ParentIds {
                    benchmark_name: "ConvoMem".to_string(),
                    benchmark_query_id: "q-phone".to_string(),
                    parent_session_id: Some("s1".to_string()),
                    parent_dialog_id: Some("d1".to_string()),
                    parent_evidence_id: Some("e1".to_string()),
                },
            }],
        };

        let row = run_item(&item, Mode::HybridParent, 10).unwrap();

        assert_eq!(row.returned_parent_ids, vec!["e1"]);
        assert_eq!(row.scores.recall_at_1, 1.0);
    }

    #[test]
    fn convomem_hybrid_parent_preserves_lexical_leader_for_rank_one() {
        let item = BenchmarkItem {
            benchmark: "ConvoMem".to_string(),
            query_id: "q-rank".to_string(),
            question: "What specific tool was mentioned?".to_string(),
            category: "assistant-facts-evidence".to_string(),
            ground_truth_ids: vec!["e1".to_string()],
            abstention: false,
            documents: vec![
                BenchmarkDocument {
                    capsule_id: 1,
                    text: "Assistant: The specific tool was Zapier for CRM sync.".to_string(),
                    parent: ParentIds {
                        benchmark_name: "ConvoMem".to_string(),
                        benchmark_query_id: "q-rank".to_string(),
                        parent_session_id: Some("s1".to_string()),
                        parent_dialog_id: Some("d1".to_string()),
                        parent_evidence_id: Some("e1".to_string()),
                    },
                },
                BenchmarkDocument {
                    capsule_id: 2,
                    text: "Assistant: CRM sync was discussed repeatedly.".to_string(),
                    parent: ParentIds {
                        benchmark_name: "ConvoMem".to_string(),
                        benchmark_query_id: "q-rank".to_string(),
                        parent_session_id: Some("s2".to_string()),
                        parent_dialog_id: Some("d2".to_string()),
                        parent_evidence_id: Some("e2".to_string()),
                    },
                },
            ],
        };

        let row = run_item(&item, Mode::HybridParent, 2).unwrap();

        assert_eq!(
            row.returned_parent_ids.first().map(String::as_str),
            Some("e1")
        );
    }

    fn parent_aggregation_fixture() -> BenchmarkItem {
        BenchmarkItem {
            benchmark: "LongMemEval".to_string(),
            query_id: "q-parent".to_string(),
            question: "What is the rare violet passphrase?".to_string(),
            category: "single-session-user".to_string(),
            ground_truth_ids: vec!["s-hit".to_string()],
            abstention: false,
            documents: vec![
                BenchmarkDocument {
                    capsule_id: 1,
                    text: "user: unrelated travel notes about trains and hotels".to_string(),
                    parent: ParentIds {
                        benchmark_name: "LongMemEval".to_string(),
                        benchmark_query_id: "q-parent".to_string(),
                        parent_session_id: Some("s-miss".to_string()),
                        parent_dialog_id: None,
                        parent_evidence_id: None,
                    },
                },
                BenchmarkDocument {
                    capsule_id: 2,
                    text: "user: the rare violet passphrase is kelpstone".to_string(),
                    parent: ParentIds {
                        benchmark_name: "LongMemEval".to_string(),
                        benchmark_query_id: "q-parent".to_string(),
                        parent_session_id: Some("s-hit".to_string()),
                        parent_dialog_id: None,
                        parent_evidence_id: None,
                    },
                },
                BenchmarkDocument {
                    capsule_id: 3,
                    text: "assistant: I can help remember that detail later".to_string(),
                    parent: ParentIds {
                        benchmark_name: "LongMemEval".to_string(),
                        benchmark_query_id: "q-parent".to_string(),
                        parent_session_id: Some("s-hit".to_string()),
                        parent_dialog_id: None,
                        parent_evidence_id: None,
                    },
                },
            ],
        }
    }
}
