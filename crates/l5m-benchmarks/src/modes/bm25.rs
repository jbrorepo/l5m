use std::collections::BTreeMap;

use crate::{
    adapters::BenchmarkItem,
    modes::{
        flat_scan::{parent_retrieval_id, terms},
        token_estimate, ReturnedCapsule,
    },
};

#[derive(Clone, Debug)]
pub(crate) struct ScoredDocument {
    pub index: usize,
    pub score: f64,
}

pub fn retrieve(item: &BenchmarkItem, top_k: usize) -> Vec<ReturnedCapsule> {
    score_documents(item)
        .into_iter()
        .take(top_k)
        .map(|scored| {
            let doc = &item.documents[scored.index];
            ReturnedCapsule {
                capsule_id: doc.capsule_id.to_string(),
                parent_id: parent_retrieval_id(item, doc),
                token_estimate: token_estimate(&doc.text),
            }
        })
        .collect()
}

pub(crate) fn score_documents(item: &BenchmarkItem) -> Vec<ScoredDocument> {
    let query_terms = terms(&item.question);
    let doc_terms = item
        .documents
        .iter()
        .map(|doc| terms(&doc.text).into_iter().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let mut document_frequency = BTreeMap::<String, usize>::new();
    for terms in &doc_terms {
        let mut seen = terms.clone();
        seen.sort();
        seen.dedup();
        for term in seen {
            *document_frequency.entry(term).or_default() += 1;
        }
    }
    let doc_count = item.documents.len() as f64;
    let avg_len = doc_terms.iter().map(Vec::len).sum::<usize>().max(1) as f64
        / item.documents.len().max(1) as f64;
    let mut scored = item
        .documents
        .iter()
        .zip(doc_terms.iter())
        .enumerate()
        .map(|(index, (doc, terms))| {
            let mut tf = BTreeMap::<&String, usize>::new();
            for term in terms {
                *tf.entry(term).or_default() += 1;
            }
            let len = terms.len().max(1) as f64;
            let score = query_terms
                .iter()
                .map(|term| {
                    let frequency = *tf.get(term).unwrap_or(&0) as f64;
                    if frequency == 0.0 {
                        return 0.0;
                    }
                    let df = *document_frequency.get(term).unwrap_or(&0) as f64;
                    let idf = ((doc_count - df + 0.5) / (df + 0.5) + 1.0).ln();
                    let k1 = 1.2;
                    let b = 0.75;
                    idf * (frequency * (k1 + 1.0))
                        / (frequency + k1 * (1.0 - b + b * len / avg_len))
                })
                .sum::<f64>();
            (index, doc, score)
        })
        .collect::<Vec<_>>();
    scored.sort_by(
        |(left_index, left_doc, left_score), (right_index, right_doc, right_score)| {
            right_score
                .partial_cmp(left_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left_doc.capsule_id.cmp(&right_doc.capsule_id))
                .then_with(|| left_index.cmp(right_index))
        },
    );
    scored
        .into_iter()
        .map(|(index, _, score)| ScoredDocument { index, score })
        .collect()
}
