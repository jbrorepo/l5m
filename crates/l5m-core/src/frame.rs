use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MemoryFrame {
    pub epoch: u64,
    pub query_hash: [u8; 32],
    pub capsules: Vec<FrameCapsule>,
    pub conflicts: Vec<FrameCapsule>,
    pub coverage: CoverageReport,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FrameCapsule {
    pub capsule_id: u128,
    pub claim: String,
    pub evidence: String,
    pub trust_level: u8,
    pub valid_from: i64,
    pub valid_until: Option<i64>,
    pub source_id: u64,
    pub source_hash: [u8; 32],
    pub relation_notes: Vec<String>,
    pub score: f32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CoverageReport {
    pub exact_entity_match: bool,
    pub anchor_match_count: usize,
    pub temporal_valid_count: usize,
    pub trust_floor_met_count: usize,
    pub context_valid_count: usize,
    pub candidate_count_before_scoring: usize,
}
