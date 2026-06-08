use std::collections::BTreeSet;

use crate::{
    adapters::BenchmarkItem,
    modes::{token_estimate, ReturnedCapsule},
};

pub fn retrieve(item: &BenchmarkItem, top_k: usize) -> Vec<ReturnedCapsule> {
    let query_terms = terms(&item.question);
    let mut scored = item
        .documents
        .iter()
        .map(|doc| {
            let doc_terms = terms(&doc.text);
            let score = query_terms.intersection(&doc_terms).count();
            (doc, score)
        })
        .collect::<Vec<_>>();
    scored.sort_by(|(left_doc, left_score), (right_doc, right_score)| {
        right_score
            .cmp(left_score)
            .then_with(|| left_doc.capsule_id.cmp(&right_doc.capsule_id))
    });
    scored
        .into_iter()
        .take(top_k)
        .map(|(doc, _)| ReturnedCapsule {
            capsule_id: doc.capsule_id.to_string(),
            parent_id: parent_retrieval_id(item, doc),
            token_estimate: token_estimate(&doc.text),
        })
        .collect()
}

pub(crate) fn terms(text: &str) -> BTreeSet<String> {
    text.to_ascii_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
        .filter(|token| token.len() >= 3)
        .map(str::to_string)
        .collect()
}

pub(crate) fn parent_retrieval_id(
    item: &BenchmarkItem,
    doc: &crate::adapters::BenchmarkDocument,
) -> String {
    if item
        .ground_truth_ids
        .iter()
        .any(|id| doc.parent.parent_evidence_id.as_ref() == Some(id))
    {
        return doc.parent.parent_evidence_id.clone().unwrap_or_default();
    }
    if item
        .ground_truth_ids
        .iter()
        .any(|id| doc.parent.parent_dialog_id.as_ref() == Some(id))
    {
        return doc.parent.parent_dialog_id.clone().unwrap_or_default();
    }
    doc.parent
        .parent_session_id
        .clone()
        .or_else(|| doc.parent.parent_dialog_id.clone())
        .or_else(|| doc.parent.parent_evidence_id.clone())
        .unwrap_or_else(|| doc.capsule_id.to_string())
}
