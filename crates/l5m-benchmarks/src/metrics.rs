use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ScoreSet {
    pub recall_at_1: f64,
    pub recall_at_5: f64,
    pub recall_at_10: f64,
    pub ndcg_at_5: f64,
    pub ndcg_at_10: f64,
    pub mrr: f64,
    pub zero_recall: bool,
}

pub fn score_retrieval(ground_truth_ids: &[String], returned_parent_ids: &[String]) -> ScoreSet {
    let truth = ground_truth_ids.iter().collect::<BTreeSet<_>>();
    if truth.is_empty() {
        return ScoreSet {
            zero_recall: returned_parent_ids.is_empty(),
            ..ScoreSet::default()
        };
    }

    let recall_at_1 = recall_at(&truth, returned_parent_ids, 1);
    let recall_at_5 = recall_at(&truth, returned_parent_ids, 5);
    let recall_at_10 = recall_at(&truth, returned_parent_ids, 10);
    ScoreSet {
        recall_at_1,
        recall_at_5,
        recall_at_10,
        ndcg_at_5: ndcg_at(&truth, returned_parent_ids, 5),
        ndcg_at_10: ndcg_at(&truth, returned_parent_ids, 10),
        mrr: mrr(&truth, returned_parent_ids),
        zero_recall: recall_at_10 == 0.0,
    }
}

fn recall_at(truth: &BTreeSet<&String>, returned: &[String], k: usize) -> f64 {
    let hits = returned
        .iter()
        .take(k)
        .filter(|id| truth.contains(id))
        .collect::<BTreeSet<_>>()
        .len();
    hits as f64 / truth.len() as f64
}

fn ndcg_at(truth: &BTreeSet<&String>, returned: &[String], k: usize) -> f64 {
    let dcg = returned
        .iter()
        .take(k)
        .enumerate()
        .filter(|(_, id)| truth.contains(id))
        .map(|(index, _)| 1.0 / ((index + 2) as f64).log2())
        .sum::<f64>();
    let ideal_hits = truth.len().min(k);
    let ideal = (0..ideal_hits)
        .map(|index| 1.0 / ((index + 2) as f64).log2())
        .sum::<f64>();
    if ideal == 0.0 {
        0.0
    } else {
        dcg / ideal
    }
}

fn mrr(truth: &BTreeSet<&String>, returned: &[String]) -> f64 {
    returned
        .iter()
        .position(|id| truth.contains(id))
        .map_or(0.0, |index| 1.0 / (index + 1) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recall_and_ndcg_are_correct() {
        let scores = score_retrieval(
            &["session-a".to_string(), "session-b".to_string()],
            &[
                "session-x".to_string(),
                "session-a".to_string(),
                "session-y".to_string(),
                "session-b".to_string(),
            ],
        );

        assert_eq!(scores.recall_at_1, 0.0);
        assert_eq!(scores.recall_at_5, 1.0);
        assert_eq!(scores.recall_at_10, 1.0);
        assert!((scores.mrr - 0.5).abs() < 0.0001);
        assert!((scores.ndcg_at_5 - 0.6509).abs() < 0.001);
        assert!(!scores.zero_recall);
    }
}
