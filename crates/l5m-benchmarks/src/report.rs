use std::collections::BTreeMap;

use crate::{latency::summarize, runfile::RunRow};

pub fn render_compare_markdown(runs: &[(String, Vec<RunRow>)]) -> String {
    let mut out = String::from("# L5M Benchmark Comparison\n\n");
    out.push_str("| Run | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall | P50 | P95 | P99 | P99.9 | Avg Candidates | Avg Scored | Avg Returned | Avg Tokens | Index Build ns | Segment Bytes |\n");
    out.push_str("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
    for (name, rows) in runs {
        let summary = summarize_rows(rows);
        out.push_str(&format!(
            "| {name} | {} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} | {} | {} | {} | {} | {:.2} | {:.2} | {:.2} | {:.2} | {} | {} |\n",
            rows.len(),
            summary.recall_at_1,
            summary.recall_at_5,
            summary.recall_at_10,
            summary.ndcg_at_5,
            summary.ndcg_at_10,
            summary.mrr,
            summary.zero_recall_rate,
            summary.p50,
            summary.p95,
            summary.p99,
            summary.p999,
            summary.avg_candidate_count_after_gates,
            summary.avg_scored_candidate_count,
            summary.avg_returned_capsule_count,
            summary.avg_returned_token_estimate,
            summary.index_build_time_ns,
            summary.segment_size_bytes
        ));
    }
    out.push_str("\n## Per-Category Breakdown\n\n");
    for (name, rows) in runs {
        out.push_str(&format!("### {name}\n\n"));
        out.push_str(
            "| Category | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR |\n",
        );
        out.push_str("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
        let mut by_category = BTreeMap::<String, Vec<RunRow>>::new();
        for row in rows {
            by_category
                .entry(row.category.clone())
                .or_default()
                .push(row.clone());
        }
        for (category, category_rows) in by_category {
            let summary = summarize_rows(&category_rows);
            out.push_str(&format!(
                "| {category} | {} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} |\n",
                category_rows.len(),
                summary.recall_at_1,
                summary.recall_at_5,
                summary.recall_at_10,
                summary.ndcg_at_5,
                summary.ndcg_at_10,
                summary.mrr
            ));
        }
        out.push('\n');
    }
    out
}

pub fn render_scorecard_markdown(preset: &str, rows: &[RunRow]) -> String {
    let summary = summarize_rows(rows);
    let gate_violations = rows
        .iter()
        .filter(|row| {
            row.gate_violations.tenant_violation
                || row.gate_violations.context_violation
                || row.gate_violations.policy_violation
                || row.gate_violations.trust_violation
                || row.gate_violations.temporal_violation
                || row.gate_violations.poison_violation
        })
        .count();
    let raw_only = rows.iter().all(|row| row.raw_retrieval_only);
    let mut out = format!("# L5M Scorecard: {preset}\n\n");
    out.push_str("## Raw Retrieval\n\n");
    out.push_str(
        "| Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall |\n",
    );
    out.push_str("| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
    out.push_str(&format!(
        "| {} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} |\n\n",
        rows.len(),
        summary.recall_at_1,
        summary.recall_at_5,
        summary.recall_at_10,
        summary.ndcg_at_5,
        summary.ndcg_at_10,
        summary.mrr,
        summary.zero_recall_rate
    ));
    out.push_str("## Hot Retrieval Latency\n\n");
    out.push_str("| P50 ns | P95 ns | P99 ns | P99.9 ns | Avg Candidates | Avg Scored | Avg Returned | Avg Tokens |\n");
    out.push_str("| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
    out.push_str(&format!(
        "| {} | {} | {} | {} | {:.2} | {:.2} | {:.2} | {:.2} |\n\n",
        summary.p50,
        summary.p95,
        summary.p99,
        summary.p999,
        summary.avg_candidate_count_after_gates,
        summary.avg_scored_candidate_count,
        summary.avg_returned_capsule_count,
        summary.avg_returned_token_estimate
    ));
    out.push_str("## Build And Size\n\n");
    out.push_str("| Build/Load ns | Segment Bytes |\n");
    out.push_str("| ---: | ---: |\n");
    out.push_str(&format!(
        "| {} | {} |\n\n",
        summary.index_build_time_ns, summary.segment_size_bytes
    ));
    out.push_str("## Gate Violations\n\n");
    out.push_str(&format!(
        "- violating rows: {gate_violations}\n- raw retrieval only: {raw_only}\n"
    ));
    out
}

pub fn render_scorecard_json(preset: &str, rows: &[RunRow]) -> serde_json::Result<String> {
    let summary = summarize_rows(rows);
    let gate_violations = rows
        .iter()
        .filter(|row| {
            row.gate_violations.tenant_violation
                || row.gate_violations.context_violation
                || row.gate_violations.policy_violation
                || row.gate_violations.trust_violation
                || row.gate_violations.temporal_violation
                || row.gate_violations.poison_violation
        })
        .count();
    serde_json::to_string_pretty(&serde_json::json!({
        "preset": preset,
        "queries": rows.len(),
        "raw_retrieval": {
            "recall_at_1": summary.recall_at_1,
            "recall_at_5": summary.recall_at_5,
            "recall_at_10": summary.recall_at_10,
            "ndcg_at_5": summary.ndcg_at_5,
            "ndcg_at_10": summary.ndcg_at_10,
            "mrr": summary.mrr,
            "zero_recall_rate": summary.zero_recall_rate
        },
        "hot_retrieval_latency_ns": {
            "p50": summary.p50,
            "p95": summary.p95,
            "p99": summary.p99,
            "p999": summary.p999
        },
        "build_and_size": {
            "index_build_time_ns": summary.index_build_time_ns,
            "segment_size_bytes": summary.segment_size_bytes
        },
        "gate_violations": gate_violations
    }))
}

pub fn render_proof_markdown(
    candidate_name: &str,
    candidate: &[RunRow],
    baseline_name: &str,
    baseline: &[RunRow],
) -> String {
    let candidate_summary = summarize_rows(candidate);
    let baseline_summary = summarize_rows(baseline);
    let accuracy_parity = candidate_summary.recall_at_1 + 1e-12 >= baseline_summary.recall_at_1
        && candidate_summary.recall_at_5 + 1e-12 >= baseline_summary.recall_at_5
        && candidate_summary.recall_at_10 + 1e-12 >= baseline_summary.recall_at_10;
    let faster = candidate_summary.p50 < baseline_summary.p50
        && candidate_summary.p95 < baseline_summary.p95
        && candidate_summary.p99 < baseline_summary.p99;
    let safety_pass = gate_violation_count(candidate) == 0;
    let same_identity = identity_tuple(candidate) == identity_tuple(baseline);
    let p50_speedup = speedup(baseline_summary.p50, candidate_summary.p50);
    let p95_speedup = speedup(baseline_summary.p95, candidate_summary.p95);
    let mut out = String::from("# L5M Proof Report\n\n");
    out.push_str("## Verdict\n\n");
    out.push_str(&format!(
        "- accuracy parity: {}\n",
        pass_fail(accuracy_parity)
    ));
    out.push_str(&format!("- latency lead: {}\n", pass_fail(faster)));
    out.push_str(&format!("- safety gates: {}\n", pass_fail(safety_pass)));
    out.push_str(&format!(
        "- same config/data/split identity: {}\n\n",
        pass_fail(same_identity)
    ));
    out.push_str("## Headline\n\n");
    out.push_str(&format!(
        "`{candidate_name}` vs `{baseline_name}`: R@1 {:.4} vs {:.4}, R@5 {:.4} vs {:.4}, R@10 {:.4} vs {:.4}; P50 {} ns vs {} ns ({:.2}x), P95 {} ns vs {} ns ({:.2}x); gate violations {}.\n\n",
        candidate_summary.recall_at_1,
        baseline_summary.recall_at_1,
        candidate_summary.recall_at_5,
        baseline_summary.recall_at_5,
        candidate_summary.recall_at_10,
        baseline_summary.recall_at_10,
        candidate_summary.p50,
        baseline_summary.p50,
        p50_speedup,
        candidate_summary.p95,
        baseline_summary.p95,
        p95_speedup,
        gate_violation_count(candidate)
    ));
    out.push_str("## Raw Metrics\n\n");
    out.push_str("| Run | Queries | R@1 | R@5 | R@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall | P50 ns | P95 ns | P99 ns |\n");
    out.push_str(
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n",
    );
    push_proof_row(
        &mut out,
        candidate_name,
        candidate.len(),
        &candidate_summary,
    );
    push_proof_row(&mut out, baseline_name, baseline.len(), &baseline_summary);
    out.push_str("\n## Identity\n\n");
    out.push_str("| Field | Candidate | Baseline |\n");
    out.push_str("| --- | --- | --- |\n");
    let candidate_identity = identity_tuple(candidate);
    let baseline_identity = identity_tuple(baseline);
    out.push_str(&format!(
        "| config_hash | {} | {} |\n",
        candidate_identity.0, baseline_identity.0
    ));
    out.push_str(&format!(
        "| dataset_hash | {} | {} |\n",
        candidate_identity.1, baseline_identity.1
    ));
    out.push_str(&format!(
        "| split_hash | {} | {} |\n",
        candidate_identity.2, baseline_identity.2
    ));
    out
}

pub fn render_proof_json(
    candidate_name: &str,
    candidate: &[RunRow],
    baseline_name: &str,
    baseline: &[RunRow],
) -> serde_json::Result<String> {
    let candidate_summary = summarize_rows(candidate);
    let baseline_summary = summarize_rows(baseline);
    serde_json::to_string_pretty(&serde_json::json!({
        "candidate": candidate_name,
        "baseline": baseline_name,
        "accuracy_parity": candidate_summary.recall_at_1 + 1e-12 >= baseline_summary.recall_at_1
            && candidate_summary.recall_at_5 + 1e-12 >= baseline_summary.recall_at_5
            && candidate_summary.recall_at_10 + 1e-12 >= baseline_summary.recall_at_10,
        "latency_lead": candidate_summary.p50 < baseline_summary.p50
            && candidate_summary.p95 < baseline_summary.p95
            && candidate_summary.p99 < baseline_summary.p99,
        "safety_pass": gate_violation_count(candidate) == 0,
        "same_identity": identity_tuple(candidate) == identity_tuple(baseline),
        "candidate_metrics": proof_metrics_json(&candidate_summary, gate_violation_count(candidate)),
        "baseline_metrics": proof_metrics_json(&baseline_summary, gate_violation_count(baseline)),
        "speedup": {
            "p50": speedup(baseline_summary.p50, candidate_summary.p50),
            "p95": speedup(baseline_summary.p95, candidate_summary.p95),
            "p99": speedup(baseline_summary.p99, candidate_summary.p99)
        }
    }))
}

#[derive(Default)]
struct RowSummary {
    recall_at_1: f64,
    recall_at_5: f64,
    recall_at_10: f64,
    ndcg_at_5: f64,
    ndcg_at_10: f64,
    mrr: f64,
    zero_recall_rate: f64,
    p50: u64,
    p95: u64,
    p99: u64,
    p999: u64,
    avg_candidate_count_after_gates: f64,
    avg_scored_candidate_count: f64,
    avg_returned_capsule_count: f64,
    avg_returned_token_estimate: f64,
    index_build_time_ns: u64,
    segment_size_bytes: u64,
}

fn proof_metrics_json(summary: &RowSummary, gate_violations: usize) -> serde_json::Value {
    serde_json::json!({
        "recall_at_1": summary.recall_at_1,
        "recall_at_5": summary.recall_at_5,
        "recall_at_10": summary.recall_at_10,
        "ndcg_at_5": summary.ndcg_at_5,
        "ndcg_at_10": summary.ndcg_at_10,
        "mrr": summary.mrr,
        "zero_recall_rate": summary.zero_recall_rate,
        "p50_ns": summary.p50,
        "p95_ns": summary.p95,
        "p99_ns": summary.p99,
        "gate_violations": gate_violations
    })
}

fn push_proof_row(out: &mut String, name: &str, count: usize, summary: &RowSummary) {
    out.push_str(&format!(
        "| {name} | {count} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} | {} | {} | {} |\n",
        summary.recall_at_1,
        summary.recall_at_5,
        summary.recall_at_10,
        summary.ndcg_at_5,
        summary.ndcg_at_10,
        summary.mrr,
        summary.zero_recall_rate,
        summary.p50,
        summary.p95,
        summary.p99
    ));
}

fn pass_fail(value: bool) -> &'static str {
    if value {
        "PASS"
    } else {
        "FAIL"
    }
}

fn speedup(baseline: u64, candidate: u64) -> f64 {
    if candidate == 0 {
        return 0.0;
    }
    baseline as f64 / candidate as f64
}

fn gate_violation_count(rows: &[RunRow]) -> usize {
    rows.iter()
        .filter(|row| {
            row.gate_violations.tenant_violation
                || row.gate_violations.context_violation
                || row.gate_violations.policy_violation
                || row.gate_violations.trust_violation
                || row.gate_violations.temporal_violation
                || row.gate_violations.poison_violation
        })
        .count()
}

fn identity_tuple(rows: &[RunRow]) -> (String, String, String) {
    rows.first()
        .map(|row| {
            (
                row.config_hash.clone(),
                row.dataset_hash.clone(),
                row.split_hash.clone(),
            )
        })
        .unwrap_or_default()
}

fn summarize_rows(rows: &[RunRow]) -> RowSummary {
    if rows.is_empty() {
        return RowSummary::default();
    }
    let count = rows.len() as f64;
    let latencies = rows
        .iter()
        .map(|row| row.timings.total_retrieval_ns)
        .collect::<Vec<_>>();
    let latency = summarize(&latencies);
    RowSummary {
        recall_at_1: rows.iter().map(|row| row.scores.recall_at_1).sum::<f64>() / count,
        recall_at_5: rows.iter().map(|row| row.scores.recall_at_5).sum::<f64>() / count,
        recall_at_10: rows.iter().map(|row| row.scores.recall_at_10).sum::<f64>() / count,
        ndcg_at_5: rows.iter().map(|row| row.scores.ndcg_at_5).sum::<f64>() / count,
        ndcg_at_10: rows.iter().map(|row| row.scores.ndcg_at_10).sum::<f64>() / count,
        mrr: rows.iter().map(|row| row.scores.mrr).sum::<f64>() / count,
        zero_recall_rate: rows.iter().filter(|row| row.scores.zero_recall).count() as f64 / count,
        p50: latency.p50,
        p95: latency.p95,
        p99: latency.p99,
        p999: latency.p999,
        avg_candidate_count_after_gates: rows
            .iter()
            .map(|row| row.candidate_counts.candidate_count_after_hard_gates)
            .sum::<usize>() as f64
            / count,
        avg_scored_candidate_count: rows
            .iter()
            .map(|row| row.candidate_counts.candidate_count_scored)
            .sum::<usize>() as f64
            / count,
        avg_returned_capsule_count: rows
            .iter()
            .map(|row| row.returned_capsule_count)
            .sum::<usize>() as f64
            / count,
        avg_returned_token_estimate: rows
            .iter()
            .map(|row| row.returned_token_estimate)
            .sum::<usize>() as f64
            / count,
        index_build_time_ns: rows.iter().map(|row| row.index_build_time_ns).sum(),
        segment_size_bytes: rows.iter().map(|row| row.segment_size_bytes).sum(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runfile::RunRow;

    #[test]
    fn compare_report_renders_markdown() {
        let rows = vec![RunRow::minimal("LongMemEval", "q1", "Where?", "bm25", 10)];
        let markdown = render_compare_markdown(&[("bm25_lme.jsonl".to_string(), rows)]);

        assert!(markdown.contains("| Run |"));
        assert!(markdown.contains("Recall@1"));
        assert!(markdown.contains("P95"));
    }

    #[test]
    fn scorecard_renders_publication_sections() {
        let rows = vec![RunRow::minimal(
            "LongMemEval",
            "q1",
            "Where?",
            "hybrid-parent",
            10,
        )];

        let markdown = render_scorecard_markdown("mempalace-longmemeval", &rows);

        assert!(markdown.contains("Raw Retrieval"));
        assert!(markdown.contains("Hot Retrieval Latency"));
        assert!(markdown.contains("Gate Violations"));
        assert!(markdown.contains("mempalace-longmemeval"));
    }

    #[test]
    fn proof_report_marks_parity_speed_and_safety() {
        let mut candidate = RunRow::minimal("LongMemEval", "q1", "Where?", "hybrid-parent", 10);
        candidate.scores.recall_at_1 = 1.0;
        candidate.scores.recall_at_5 = 1.0;
        candidate.scores.recall_at_10 = 1.0;
        candidate.timings.total_retrieval_ns = 10;
        candidate.config_hash = "cfg".to_string();
        candidate.dataset_hash = "data".to_string();
        candidate.split_hash = "split".to_string();
        let mut baseline = candidate.clone();
        baseline.mode = "bm25".to_string();
        baseline.timings.total_retrieval_ns = 20;

        let markdown = render_proof_markdown("hybrid", &[candidate], "bm25", &[baseline]);

        assert!(markdown.contains("accuracy parity: PASS"));
        assert!(markdown.contains("latency lead: PASS"));
        assert!(markdown.contains("safety gates: PASS"));
    }
}
