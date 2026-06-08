use std::error::Error;

use crate::{
    adapters::{BenchmarkDocument, BenchmarkItem, ParentIds},
    modes::{l5m_session_verbatim::retrieve_documents_with_l5m, ModeRun},
};

#[derive(Clone, Debug, PartialEq)]
pub struct BenchmarkCapsule {
    pub capsule_id: u128,
    pub text: String,
    pub parent: ParentIds,
}

pub fn build_capsules(documents: &[BenchmarkDocument]) -> Vec<BenchmarkCapsule> {
    documents
        .iter()
        .map(|doc| BenchmarkCapsule {
            capsule_id: doc.capsule_id,
            text: doc.text.clone(),
            parent: doc.parent.clone(),
        })
        .collect()
}

pub fn retrieve(item: &BenchmarkItem, top_k: usize) -> Result<ModeRun, Box<dyn Error>> {
    let _capsules = build_capsules(&item.documents);
    retrieve_documents_with_l5m(item, &item.documents, top_k, None)
}
