use std::error::Error;

use crate::{
    adapters::BenchmarkItem,
    modes::{l5m_session_verbatim::retrieve_documents_with_l5m, ModeRun},
};

pub fn retrieve(item: &BenchmarkItem, top_k: usize) -> Result<ModeRun, Box<dyn Error>> {
    retrieve_documents_with_l5m(item, &item.documents, top_k, None)
}
