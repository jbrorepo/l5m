use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use clap::ValueEnum;
use serde_json::Value;

use crate::modes::ReturnedCapsule;

use super::{
    normalize_category, root_items, string_field, string_list_field, text_from_value,
    AdapterResult, BenchmarkDocument, BenchmarkItem, ParentIds,
};

const CATEGORIES: &[&str] = &[
    "user-evidence",
    "assistant-facts-evidence",
    "changing-evidence",
    "abstention-evidence",
    "preference-evidence",
    "implicit-connection-evidence",
];

pub fn parse_path(
    path: &Path,
    categories: &[String],
    limit: Option<usize>,
    layout: ConvoMemLayout,
) -> AdapterResult<Vec<BenchmarkItem>> {
    let mut out = Vec::new();
    if path.is_dir() {
        for entry_path in json_files(path, layout)? {
            let inferred_category = infer_category_from_path(&entry_path);
            out.extend(parse_str_with_default_category(
                &fs::read_to_string(entry_path)?,
                categories,
                inferred_category.as_deref(),
            )?);
        }
    } else {
        out.extend(parse_str_with_default_category(
            &fs::read_to_string(path)?,
            categories,
            infer_category_from_path(path).as_deref(),
        )?);
    }
    if let Some(limit) = limit {
        out.truncate(limit);
    }
    Ok(out)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ConvoMemLayout {
    Auto,
    Full,
    Flat,
}

#[cfg(test)]
pub fn parse_str(input: &str, categories: &[String]) -> AdapterResult<Vec<BenchmarkItem>> {
    parse_str_with_default_category(input, categories, None)
}

fn parse_str_with_default_category(
    input: &str,
    categories: &[String],
    default_category: Option<&str>,
) -> AdapterResult<Vec<BenchmarkItem>> {
    let wanted = normalize_requested_categories(categories);
    let value: Value = serde_json::from_str(input)?;
    let mut out = Vec::new();
    for (index, item) in convomem_items(&value).into_iter().enumerate() {
        let category = benchmark_category(item, default_category);
        if !wanted.is_empty() && !wanted.contains(&category) {
            continue;
        }
        let query_id = string_field(item, &["question_id", "query_id", "id"])
            .unwrap_or_else(|| stable_query_id(index, item));
        let question = string_field(item, &["question", "query"]).unwrap_or_default();
        let (documents, evidence_id_by_key) = documents_from_item(index, item, &query_id);
        let ground_truth_ids = if category == "abstention-evidence" {
            Vec::new()
        } else {
            ground_truth_ids_from_item(item, &evidence_id_by_key)
        };
        out.push(BenchmarkItem {
            benchmark: "ConvoMem".to_string(),
            query_id,
            question,
            category: category.clone(),
            documents,
            ground_truth_ids,
            abstention: category == "abstention-evidence",
        });
    }
    Ok(out)
}

fn convomem_items(value: &Value) -> Vec<&Value> {
    if let Some(items) = value.get("evidence_items").and_then(Value::as_array) {
        return items.iter().collect();
    }
    root_items(value)
}

fn benchmark_category(item: &Value, default_category: Option<&str>) -> String {
    let from_path = default_category.map(normalize_category);
    if from_path
        .as_deref()
        .is_some_and(|category| CATEGORIES.contains(&category))
    {
        return from_path.unwrap();
    }
    string_field(
        item,
        &["evidence_type", "question_type", "category", "type"],
    )
    .map(|value| normalize_category(&value))
    .unwrap_or_else(|| from_path.unwrap_or_else(|| "unknown".to_string()))
}

fn documents_from_item(
    item_index: usize,
    item: &Value,
    query_id: &str,
) -> (Vec<BenchmarkDocument>, BTreeMap<String, String>) {
    let mut raw_docs = explicit_docs(item);
    if raw_docs.is_empty() {
        raw_docs = conversation_message_docs(item);
    }
    let mut evidence_id_by_key = BTreeMap::new();
    let documents = raw_docs
        .into_iter()
        .enumerate()
        .map(|(doc_index, raw)| {
            let evidence_id = raw
                .evidence_id
                .unwrap_or_else(|| format!("{query_id}-e-{doc_index}"));
            evidence_id_by_key.insert(
                normalize_message_key(&raw.speaker, &raw.text),
                evidence_id.clone(),
            );
            BenchmarkDocument {
                capsule_id: (item_index as u128 + 1) * 1_000_000 + doc_index as u128 + 1,
                text: if raw.speaker.is_empty() {
                    raw.text
                } else {
                    format!("{}: {}", raw.speaker, raw.text)
                },
                parent: ParentIds {
                    benchmark_name: "ConvoMem".to_string(),
                    benchmark_query_id: query_id.to_string(),
                    parent_session_id: raw.session_id,
                    parent_dialog_id: raw.dialog_id,
                    parent_evidence_id: Some(evidence_id),
                },
            }
        })
        .collect();
    (documents, evidence_id_by_key)
}

#[derive(Clone)]
struct RawDoc {
    evidence_id: Option<String>,
    speaker: String,
    text: String,
    session_id: Option<String>,
    dialog_id: Option<String>,
}

fn explicit_docs(item: &Value) -> Vec<RawDoc> {
    item.get("evidence")
        .or_else(|| item.get("documents"))
        .or_else(|| item.get("memories"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|doc| RawDoc {
            evidence_id: string_field(doc, &["evidence_id", "id"]),
            speaker: string_field(doc, &["speaker", "role"]).unwrap_or_default(),
            text: text_from_value(doc),
            session_id: string_field(doc, &["session_id", "conversation_id"]),
            dialog_id: string_field(doc, &["dialog_id", "turn_id"]),
        })
        .collect()
}

fn conversation_message_docs(item: &Value) -> Vec<RawDoc> {
    let mut docs = Vec::new();
    for (conversation_index, conversation) in item
        .get("conversations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let session_id = string_field(conversation, &["session_id", "conversation_id", "id"])
            .unwrap_or_else(|| format!("conversation-{conversation_index}"));
        for (message_index, message) in conversation
            .get("messages")
            .or_else(|| conversation.get("turns"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            docs.push(RawDoc {
                evidence_id: None,
                speaker: string_field(message, &["speaker", "role"]).unwrap_or_default(),
                text: string_field(message, &["text", "content", "message"]).unwrap_or_default(),
                session_id: Some(session_id.clone()),
                dialog_id: Some(format!("{session_id}:{message_index}")),
            });
        }
    }
    docs
}

fn ground_truth_ids_from_item(
    item: &Value,
    evidence_id_by_key: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut ids = string_list_field(
        item,
        &["ground_truth_ids", "evidence_ids", "answer_evidence_ids"],
    )
    .into_iter()
    .collect::<BTreeSet<_>>();
    for evidence in item
        .get("message_evidences")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let speaker = string_field(evidence, &["speaker", "role"]).unwrap_or_default();
        let text = string_field(evidence, &["text", "content", "message"]).unwrap_or_default();
        if let Some(id) = evidence_id_by_key.get(&normalize_message_key(&speaker, &text)) {
            ids.insert(id.clone());
        }
    }
    ids.into_iter().collect()
}

fn normalize_message_key(speaker: &str, text: &str) -> String {
    format!(
        "{}\n{}",
        speaker.trim().to_ascii_lowercase(),
        text.trim().to_ascii_lowercase()
    )
}

fn stable_query_id(index: usize, item: &Value) -> String {
    let question = string_field(item, &["question", "query"]).unwrap_or_else(|| item.to_string());
    let hash = blake3::hash(question.as_bytes());
    format!("convomem-{index}-{}", &hex32(hash.as_bytes())[0..12])
}

fn hex32(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub fn score_abstention(returned: &[ReturnedCapsule]) -> bool {
    returned.is_empty()
        || returned
            .iter()
            .any(|capsule| capsule.parent_id == "insufficient-evidence")
}

pub fn supported_categories() -> &'static [&'static str] {
    CATEGORIES
}

fn normalize_requested_categories(categories: &[String]) -> Vec<String> {
    if categories.is_empty() || categories.iter().any(|category| category == "all") {
        return Vec::new();
    }
    categories
        .iter()
        .map(|category| normalize_category(category))
        .collect()
}

fn json_files(path: &Path, layout: ConvoMemLayout) -> AdapterResult<Vec<std::path::PathBuf>> {
    let mut out = Vec::new();
    collect_json_files(
        path,
        matches!(layout, ConvoMemLayout::Full | ConvoMemLayout::Auto),
        &mut out,
    )?;
    out.sort();
    Ok(out)
}

fn collect_json_files(
    path: &Path,
    recursive: bool,
    out: &mut Vec<std::path::PathBuf>,
) -> AdapterResult<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        if entry_path.is_dir() && recursive {
            collect_json_files(&entry_path, recursive, out)?;
        } else if entry_path.extension().and_then(|value| value.to_str()) == Some("json") {
            out.push(entry_path);
        }
    }
    Ok(())
}

fn infer_category_from_path(path: &Path) -> Option<String> {
    path.components()
        .filter_map(|component| component.as_os_str().to_str())
        .map(normalize_category)
        .find(|component| CATEGORIES.contains(&component.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abstention_can_succeed_when_no_evidence_is_returned() {
        assert!(score_abstention(&[]));
    }

    #[test]
    fn abstention_can_succeed_with_explicit_insufficient_evidence() {
        assert!(score_abstention(&[ReturnedCapsule {
            capsule_id: "none".to_string(),
            parent_id: "insufficient-evidence".to_string(),
            token_estimate: 0,
        }]));
    }

    #[test]
    fn full_layout_parser_maps_all_six_categories() {
        let input = r#"{
          "items": [
            { "id": "q1", "question": "u", "category": "user evidence", "evidence": [{ "id": "e1", "text": "u" }], "evidence_ids": ["e1"] },
            { "id": "q2", "question": "a", "category": "assistant facts evidence", "evidence": [{ "id": "e2", "text": "a" }], "evidence_ids": ["e2"] },
            { "id": "q3", "question": "c", "category": "changing evidence", "evidence": [{ "id": "e3", "text": "c" }], "evidence_ids": ["e3"] },
            { "id": "q4", "question": "n", "category": "abstention evidence", "evidence": [], "evidence_ids": [] },
            { "id": "q5", "question": "p", "category": "preference evidence", "evidence": [{ "id": "e5", "text": "p" }], "evidence_ids": ["e5"] },
            { "id": "q6", "question": "i", "category": "implicit connection evidence", "evidence": [{ "id": "e6", "text": "i" }], "evidence_ids": ["e6"] }
          ]
        }"#;

        let items = parse_str(input, &[String::from("all")]).unwrap();
        let categories = items
            .iter()
            .map(|item| item.category.as_str())
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(categories.len(), 6);
        assert!(items.iter().any(|item| item.abstention));
    }

    #[test]
    fn real_evidence_items_layout_maps_message_evidence_to_document_id() {
        let input = r#"{
          "evidence_items": [
            {
              "question": "Which tool?",
              "answer": "Zapier",
              "message_evidences": [
                { "speaker": "Assistant", "text": "Use Zapier for the sync." }
              ],
              "conversations": [
                {
                  "messages": [
                    { "speaker": "User", "text": "I need CRM sync." },
                    { "speaker": "Assistant", "text": "Use Zapier for the sync." }
                  ]
                }
              ]
            }
          ]
        }"#;

        let items = parse_str_with_default_category(
            input,
            &[String::from("all")],
            Some("assistant-facts-evidence"),
        )
        .unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].documents.len(), 2);
        assert_eq!(
            items[0].ground_truth_ids,
            vec![items[0].documents[1]
                .parent
                .parent_evidence_id
                .clone()
                .unwrap()]
        );
    }

    #[test]
    fn folder_evidence_category_takes_precedence_over_domain_category() {
        let input = r#"{
          "evidence_items": [
            {
              "question": "Which tool?",
              "category": "professional_life",
              "message_evidences": [],
              "conversations": []
            }
          ]
        }"#;

        let items = parse_str_with_default_category(
            input,
            &[String::from("all")],
            Some("assistant_facts_evidence"),
        )
        .unwrap();

        assert_eq!(items[0].category, "assistant-facts-evidence");
    }

    #[test]
    fn abstention_layout_does_not_treat_related_messages_as_ground_truth() {
        let input = r#"{
          "evidence_items": [
            {
              "question": "What is the direct phone number?",
              "message_evidences": [
                { "speaker": "User", "text": "The lead was promising." }
              ],
              "conversations": [
                {
                  "messages": [
                    { "speaker": "User", "text": "The lead was promising." }
                  ]
                }
              ]
            }
          ]
        }"#;

        let items = parse_str_with_default_category(
            input,
            &[String::from("all")],
            Some("abstention-evidence"),
        )
        .unwrap();

        assert!(items[0].ground_truth_ids.is_empty());
        assert!(items[0].abstention);
    }
}
