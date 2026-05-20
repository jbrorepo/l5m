use crate::relation::RelationEdge;

#[derive(Clone, Debug)]
pub struct MemoryCapsule {
    pub capsule_id: u128,
    pub tenant_id: u64,
    pub claim: String,
    pub evidence: String,
    pub source_id: u64,
    pub source_uri: Option<String>,
    pub source_hash: [u8; 32],
    pub semantic_bits: [u64; 4],
    pub residual: [i8; 64],
    pub anchors: Vec<String>,
    pub entities: Vec<String>,
    pub valid_from: i64,
    pub valid_until: Option<i64>,
    pub observed_at: i64,
    pub last_verified_at: i64,
    pub context_mask: u128,
    pub policy_mask: u128,
    pub trust_level: u8,
    pub classification: u8,
    pub poison_risk: u8,
    pub relation_edges: Vec<RelationEdge>,
    pub content_hash: [u8; 32],
}
