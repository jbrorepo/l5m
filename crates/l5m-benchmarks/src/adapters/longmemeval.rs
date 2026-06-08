use std::{collections::BTreeSet, fs, path::Path};

use serde_json::Value;

use super::{
    normalize_category, root_items, string_field, string_list_field, text_from_value,
    AdapterResult, BenchmarkDocument, BenchmarkItem, ParentIds,
};

pub fn parse_path(path: &Path) -> AdapterResult<Vec<BenchmarkItem>> {
    parse_str(&fs::read_to_string(path)?)
}

pub fn parse_str(input: &str) -> AdapterResult<Vec<BenchmarkItem>> {
    let value: Value = serde_json::from_str(input)?;
    let mut out = Vec::new();
    for (item_index, item) in root_items(&value).into_iter().enumerate() {
        let query_id = string_field(
            item,
            &["question_id", "query_id", "qid", "id", "questionId"],
        )
        .unwrap_or_else(|| format!("lme-q-{item_index}"));
        let question =
            string_field(item, &["question", "query", "input"]).unwrap_or_else(|| item.to_string());
        let category = string_field(item, &["category", "type", "question_type"])
            .map(|value| normalize_category(&value))
            .unwrap_or_else(|| "unknown".to_string());
        let sessions = item
            .get("haystack_sessions")
            .or_else(|| item.get("sessions"))
            .or_else(|| item.get("memory"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let session_ids = string_list_field(item, &["haystack_session_ids", "session_ids"]);

        let mut documents = Vec::new();
        for (session_index, session) in sessions.iter().enumerate() {
            let session_id = string_field(session, &["session_id", "sessionId", "id"])
                .or_else(|| session_ids.get(session_index).cloned())
                .unwrap_or_else(|| format!("session-{session_index}"));
            documents.push(BenchmarkDocument {
                capsule_id: (item_index as u128 + 1) * 1_000_000 + session_index as u128 + 1,
                text: text_from_value(session),
                parent: ParentIds {
                    benchmark_name: "LongMemEval".to_string(),
                    benchmark_query_id: query_id.clone(),
                    parent_session_id: Some(session_id),
                    parent_dialog_id: None,
                    parent_evidence_id: None,
                },
            });
        }

        let ground_truth_ids = collect_ground_truth_session_ids(item);
        out.push(BenchmarkItem {
            benchmark: "LongMemEval".to_string(),
            query_id,
            question,
            category,
            documents,
            ground_truth_ids,
            abstention: false,
        });
    }
    Ok(out)
}

fn collect_ground_truth_session_ids(item: &Value) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for id in string_list_field(
        item,
        &[
            "ground_truth_ids",
            "gold_session_ids",
            "evidence_session_ids",
            "answer_session_ids",
            "target_session_ids",
            "session_ids",
        ],
    ) {
        ids.insert(id);
    }
    for evidence in item
        .get("evidence")
        .or_else(|| item.get("evidences"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(id) = string_field(evidence, &["session_id", "sessionId", "parent_session_id"])
        {
            ids.insert(id);
        }
    }
    ids.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use crate::{metrics::score_retrieval, modes::ReturnedCapsule};

    #[test]
    fn returned_parent_session_id_scores_against_ground_truth() {
        let returned = [ReturnedCapsule {
            capsule_id: "100".to_string(),
            parent_id: "session-7".to_string(),
            token_estimate: 4,
        }];
        let parent_ids = returned
            .iter()
            .map(|capsule| capsule.parent_id.clone())
            .collect::<Vec<_>>();

        let scores = score_retrieval(&["session-7".to_string()], &parent_ids);

        assert_eq!(scores.recall_at_1, 1.0);
        assert_eq!(scores.mrr, 1.0);
    }
}
