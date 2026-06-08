pub mod convomem;
pub mod locomo;
pub mod longmemeval;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ParentIds {
    pub benchmark_name: String,
    pub benchmark_query_id: String,
    pub parent_session_id: Option<String>,
    pub parent_dialog_id: Option<String>,
    pub parent_evidence_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct BenchmarkDocument {
    pub capsule_id: u128,
    pub text: String,
    pub parent: ParentIds,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct BenchmarkItem {
    pub benchmark: String,
    pub query_id: String,
    pub question: String,
    pub category: String,
    pub documents: Vec<BenchmarkDocument>,
    pub ground_truth_ids: Vec<String>,
    pub abstention: bool,
}

pub type AdapterResult<T> = Result<T, Box<dyn std::error::Error>>;

pub(crate) fn root_items(value: &Value) -> Vec<&Value> {
    if let Some(array) = value.as_array() {
        return array.iter().collect();
    }
    ["data", "examples", "questions", "conversations", "items"]
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_array))
        .map(|array| array.iter().collect())
        .unwrap_or_else(|| vec![value])
}

pub(crate) fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key))
        .and_then(value_to_string)
}

pub(crate) fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

pub(crate) fn string_list_field(value: &Value, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .filter_map(|key| value.get(*key))
        .flat_map(value_to_string_list)
        .collect()
}

pub(crate) fn value_to_string_list(value: &Value) -> Vec<String> {
    match value {
        Value::Array(array) => array.iter().filter_map(value_to_string).collect(),
        Value::String(text) => vec![text.clone()],
        Value::Object(object) => object
            .values()
            .flat_map(value_to_string_list)
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    }
}

pub(crate) fn text_from_value(value: &Value) -> String {
    if let Value::Array(array) = value {
        return array
            .iter()
            .map(text_from_value)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
    }
    if let Some(text) = string_field(value, &["text", "content", "transcript", "message"]) {
        let speaker = string_field(value, &["speaker", "role"]).unwrap_or_default();
        return if speaker.is_empty() {
            text
        } else {
            format!("{speaker}: {text}")
        };
    }
    if let Some(turns) = value
        .get("turns")
        .or_else(|| value.get("dialogs"))
        .or_else(|| value.get("messages"))
        .and_then(Value::as_array)
    {
        return turns
            .iter()
            .map(|turn| {
                let speaker = string_field(turn, &["speaker", "role"]).unwrap_or_default();
                let text = string_field(turn, &["text", "content", "message"]).unwrap_or_default();
                if speaker.is_empty() {
                    text
                } else {
                    format!("{speaker}: {text}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    value.to_string()
}

pub(crate) fn normalize_category(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['_', ' '], "-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locomo_dialog_evidence_maps_to_session_evidence() {
        let input = r#"[
          {
            "conversation_id": "c1",
            "sessions": [
              {
                "session_id": "s1",
                "dialogs": [
                  { "dialog_id": "d1", "speaker": "user", "text": "I live in Austin." },
                  { "dialog_id": "d2", "speaker": "assistant", "text": "Noted." }
                ]
              }
            ],
            "qa": [
              {
                "question_id": "q1",
                "question": "Where does the user live?",
                "category": "single-hop",
                "evidence_dialog_ids": ["d1"]
              }
            ]
          }
        ]"#;

        let items = locomo::parse_str(input, locomo::Granularity::Session).unwrap();
        assert_eq!(items[0].ground_truth_ids, vec!["s1".to_string()]);
    }
}
