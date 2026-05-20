use std::{collections::BTreeMap, fs, path::Path};

use clap::ValueEnum;
use serde_json::Value;

use super::{
    normalize_category, root_items, string_field, string_list_field, text_from_value,
    AdapterResult, BenchmarkDocument, BenchmarkItem, ParentIds,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Granularity {
    Session,
    Dialog,
}

pub fn parse_path(path: &Path, granularity: Granularity) -> AdapterResult<Vec<BenchmarkItem>> {
    parse_str(&fs::read_to_string(path)?, granularity)
}

pub fn parse_str(input: &str, granularity: Granularity) -> AdapterResult<Vec<BenchmarkItem>> {
    let value: Value = serde_json::from_str(input)?;
    let mut out = Vec::new();
    for (conversation_index, conversation) in root_items(&value).into_iter().enumerate() {
        let conversation_id =
            string_field(conversation, &["conversation_id", "conversationId", "id"])
                .unwrap_or_else(|| format!("conversation-{conversation_index}"));
        let sessions = collect_sessions(conversation);
        let (documents, dialog_to_session) =
            build_documents(&conversation_id, &sessions, granularity, conversation_index);
        let qa_pairs = conversation
            .get("qa")
            .or_else(|| conversation.get("qa_pairs"))
            .or_else(|| conversation.get("questions"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (qa_index, qa) in qa_pairs.iter().enumerate() {
            let query_id = string_field(qa, &["question_id", "query_id", "qid", "id"])
                .unwrap_or_else(|| format!("{conversation_id}-q-{qa_index}"));
            let question = string_field(qa, &["question", "query"]).unwrap_or_default();
            let category = string_field(qa, &["category", "type"])
                .map(|value| normalize_locomo_category(&value))
                .unwrap_or_else(|| "unknown".to_string());
            let evidence = evidence_ids(qa, granularity, &dialog_to_session);
            out.push(BenchmarkItem {
                benchmark: "LoCoMo".to_string(),
                query_id,
                question,
                category,
                documents: documents.clone(),
                ground_truth_ids: evidence,
                abstention: false,
            });
        }
    }
    Ok(out)
}

fn build_documents(
    conversation_id: &str,
    sessions: &[(String, Value)],
    granularity: Granularity,
    conversation_index: usize,
) -> (Vec<BenchmarkDocument>, BTreeMap<String, String>) {
    let mut docs = Vec::new();
    let mut dialog_to_session = BTreeMap::new();
    for (session_index, (fallback_session_id, session)) in sessions.iter().enumerate() {
        let session_id = string_field(session, &["session_id", "sessionId", "id"])
            .unwrap_or_else(|| fallback_session_id.clone());
        let dialogs = session
            .get("dialogs")
            .or_else(|| session.get("turns"))
            .or_else(|| session.get("messages"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (dialog_index, dialog) in dialogs.iter().enumerate() {
            let dialog_id = string_field(dialog, &["dialog_id", "dialogId", "id"])
                .unwrap_or_else(|| format!("{session_id}-d-{dialog_index}"));
            dialog_to_session.insert(dialog_id, session_id.clone());
        }
        match granularity {
            Granularity::Session => docs.push(BenchmarkDocument {
                capsule_id: (conversation_index as u128 + 1) * 1_000_000
                    + session_index as u128
                    + 1,
                text: text_from_value(session),
                parent: ParentIds {
                    benchmark_name: "LoCoMo".to_string(),
                    benchmark_query_id: conversation_id.to_string(),
                    parent_session_id: Some(session_id),
                    parent_dialog_id: None,
                    parent_evidence_id: None,
                },
            }),
            Granularity::Dialog => {
                for (dialog_index, dialog) in dialogs.iter().enumerate() {
                    let dialog_id = string_field(dialog, &["dialog_id", "dialogId", "id"])
                        .unwrap_or_else(|| format!("{session_id}-d-{dialog_index}"));
                    docs.push(BenchmarkDocument {
                        capsule_id: (conversation_index as u128 + 1) * 1_000_000
                            + (session_index as u128 + 1) * 10_000
                            + dialog_index as u128
                            + 1,
                        text: text_from_value(dialog),
                        parent: ParentIds {
                            benchmark_name: "LoCoMo".to_string(),
                            benchmark_query_id: conversation_id.to_string(),
                            parent_session_id: Some(session_id.clone()),
                            parent_dialog_id: Some(dialog_id.clone()),
                            parent_evidence_id: Some(dialog_id),
                        },
                    });
                }
            }
        }
    }
    (docs, dialog_to_session)
}

fn collect_sessions(conversation: &Value) -> Vec<(String, Value)> {
    if let Some(sessions) = conversation
        .get("sessions")
        .or_else(|| conversation.get("chunks"))
        .and_then(Value::as_array)
    {
        return sessions
            .iter()
            .enumerate()
            .map(|(index, session)| {
                (
                    string_field(session, &["session_id", "sessionId", "id"])
                        .unwrap_or_else(|| format!("session_{}", index + 1)),
                    session.clone(),
                )
            })
            .collect();
    }

    let Some(object) = conversation.get("conversation").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut sessions = object
        .iter()
        .filter_map(|(key, value)| {
            key.strip_prefix("session_")
                .filter(|suffix| suffix.chars().all(|ch| ch.is_ascii_digit()))
                .map(|_| (key.clone(), value.clone()))
        })
        .collect::<Vec<_>>();
    sessions.sort_by_key(|(key, _)| {
        key.strip_prefix("session_")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(usize::MAX)
    });
    sessions
}

fn evidence_ids(
    qa: &Value,
    granularity: Granularity,
    dialog_to_session: &BTreeMap<String, String>,
) -> Vec<String> {
    let raw = string_list_field(
        qa,
        &[
            "evidence_ids",
            "evidence",
            "evidence_dialog_ids",
            "dialog_evidence_ids",
            "evidence_session_ids",
            "session_evidence_ids",
        ],
    );
    let mut ids = raw
        .into_iter()
        .filter_map(|id| match granularity {
            Granularity::Dialog => Some(id),
            Granularity::Session => dialog_to_session
                .get(&id)
                .cloned()
                .or_else(|| {
                    id.split_once(':')
                        .map(|(session, _)| locomo_session_name(session))
                })
                .or(Some(id)),
        })
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    ids
}

fn locomo_session_name(dialog_session: &str) -> String {
    dialog_session
        .strip_prefix('D')
        .filter(|suffix| suffix.chars().all(|ch| ch.is_ascii_digit()))
        .map_or_else(
            || dialog_session.to_string(),
            |suffix| format!("session_{suffix}"),
        )
}

fn normalize_locomo_category(value: &str) -> String {
    match value.trim() {
        "1" => "single-hop".to_string(),
        "2" => "temporal".to_string(),
        "3" => "temporal-inference".to_string(),
        "4" => "open-domain".to_string(),
        "5" => "adversarial".to_string(),
        other => normalize_category(other),
    }
}
