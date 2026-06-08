use crate::runfile::RunRow;

pub fn render_audit_markdown(rows: &[RunRow]) -> String {
    let mut out = String::from("# L5M Retrieval Audit\n\n");
    out.push_str("| Query | Category | Mode | Zero Recall | Missed IDs | Hit Ranks | Candidates | Returned | Total ns |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- | ---: | ---: | ---: |\n");
    for row in rows {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            row.query_id,
            row.category,
            row.mode,
            if row.scores.zero_recall { "yes" } else { "no" },
            row.missed_ground_truth_ids.join(", "),
            row.hit_ranks
                .iter()
                .map(|rank| rank.to_string())
                .collect::<Vec<_>>()
                .join(", "),
            row.candidate_counts.candidate_count_initial,
            row.returned_parent_ids.len(),
            row.timings.total_retrieval_ns
        ));
    }
    out
}

pub fn explain_miss(rows: &[RunRow], query_id: &str) -> Option<String> {
    let row = rows.iter().find(|row| row.query_id == query_id)?;
    let mut out = String::new();
    out.push_str(&format!(
        "# Retrieval Miss Explanation: {}\n\n",
        row.query_id
    ));
    out.push_str(&format!("- benchmark: {}\n", row.benchmark));
    out.push_str(&format!("- category: {}\n", row.category));
    out.push_str(&format!("- mode: {}\n", row.mode));
    out.push_str(&format!(
        "- retrieval granularity: {}\n",
        row.retrieval_granularity
    ));
    out.push_str(&format!(
        "- candidate count: {}\n",
        row.candidate_counts.candidate_count_initial
    ));
    out.push_str(&format!(
        "- total retrieval ns: {}\n",
        row.timings.total_retrieval_ns
    ));
    out.push_str(&format!(
        "- ground truth IDs: {}\n",
        row.ground_truth_ids.join(", ")
    ));
    out.push_str(&format!(
        "- returned parent IDs: {}\n",
        row.returned_parent_ids.join(", ")
    ));
    out.push_str(&format!(
        "- missed ground truth IDs: {}\n",
        row.missed_ground_truth_ids.join(", ")
    ));
    let ranks = row
        .hit_ranks
        .iter()
        .map(|rank| format!("rank {rank}"))
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!("- hit ranks: {ranks}\n"));
    Some(out)
}

pub fn render_diagnose_markdown(rows: &[RunRow]) -> String {
    let buckets = diagnose_rows(rows);
    let mut out = String::from("# L5M Retrieval Diagnosis\n\n");
    out.push_str("| Bucket | Count |\n");
    out.push_str("| --- | ---: |\n");
    for (bucket, count) in buckets {
        out.push_str(&format!("| {bucket} | {count} |\n"));
    }
    out.push_str("\n## Misses\n\n");
    out.push_str("| Query | Category | Bucket | Missed IDs | Hit Ranks | Returned |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- |\n");
    for row in rows
        .iter()
        .filter(|row| !row.missed_ground_truth_ids.is_empty())
    {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            row.query_id,
            row.category,
            diagnose_row(row),
            row.missed_ground_truth_ids.join(", "),
            row.hit_ranks
                .iter()
                .map(|rank| rank.to_string())
                .collect::<Vec<_>>()
                .join(", "),
            row.returned_parent_ids.join(", ")
        ));
    }
    out
}

fn diagnose_rows(rows: &[RunRow]) -> Vec<(&'static str, usize)> {
    let mut buckets = std::collections::BTreeMap::<&'static str, usize>::new();
    for row in rows {
        *buckets.entry(diagnose_row(row)).or_default() += 1;
    }
    buckets.into_iter().collect()
}

fn diagnose_row(row: &RunRow) -> &'static str {
    if row.missed_ground_truth_ids.is_empty() {
        "hit"
    } else if row.candidate_pool_exhausted || row.retrieves_all_mode {
        "candidate-pool-exhausted"
    } else if !row.hit_ranks.is_empty() {
        "multi-session-partial"
    } else if row.candidate_counts.candidate_count_before_scoring == 0 {
        "filtered-by-narrowing"
    } else if row.scores.zero_recall {
        "zero-recall"
    } else {
        "below-top-k"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runfile::RunRow;

    #[test]
    fn explain_miss_reports_missing_ids_and_hit_ranks() {
        let mut row = RunRow::minimal("LongMemEval", "q42", "Where?", "hybrid-parent", 10);
        row.category = "multi-session".to_string();
        row.ground_truth_ids = vec!["s1".to_string(), "s2".to_string()];
        row.returned_parent_ids = vec!["s3".to_string(), "s1".to_string()];
        row.candidate_counts.candidate_count_initial = 5;
        row.timings.total_retrieval_ns = 123;
        row.missed_ground_truth_ids = vec!["s2".to_string()];
        row.hit_ranks = vec![2];

        let explanation = explain_miss(&[row], "q42").unwrap();

        assert!(explanation.contains("q42"));
        assert!(explanation.contains("multi-session"));
        assert!(explanation.contains("s2"));
        assert!(explanation.contains("rank 2"));
    }

    #[test]
    fn audit_markdown_marks_zero_recall_rows() {
        let mut row = RunRow::minimal("LoCoMo", "q1", "When?", "bm25", 10);
        row.scores.zero_recall = true;
        row.missed_ground_truth_ids = vec!["session_1".to_string()];

        let markdown = render_audit_markdown(&[row]);

        assert!(markdown.contains("| q1 |"));
        assert!(markdown.contains("yes"));
        assert!(markdown.contains("session_1"));
    }

    #[test]
    fn diagnose_markdown_buckets_known_miss() {
        let mut row = RunRow::minimal("LongMemEval", "q1", "Where?", "hybrid-parent", 10);
        row.scores.zero_recall = true;
        row.missed_ground_truth_ids = vec!["s1".to_string()];
        row.candidate_counts.candidate_count_before_scoring = 4;

        let markdown = render_diagnose_markdown(&[row]);

        assert!(markdown.contains("zero-recall"));
        assert!(markdown.contains("q1"));
    }
}
