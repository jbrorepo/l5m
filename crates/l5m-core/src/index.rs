use std::collections::HashMap;

use crate::{MemoryCapsule, RelationKind};

#[derive(Clone, Debug, Default)]
pub struct SegmentIndex {
    pub anchors: HashMap<u64, Vec<usize>>,
    pub entities: HashMap<u64, Vec<usize>>,
    pub semantic_buckets: HashMap<u16, Vec<usize>>,
    pub supersedes_by_target: HashMap<u128, Vec<usize>>,
    pub by_id: HashMap<u128, usize>,
}

impl SegmentIndex {
    pub fn build(capsules: &[MemoryCapsule]) -> Self {
        let mut index = Self::default();
        for (ordinal, capsule) in capsules.iter().enumerate() {
            index.by_id.insert(capsule.capsule_id, ordinal);
            for anchor in &capsule.anchors {
                index
                    .anchors
                    .entry(stable_hash64(anchor))
                    .or_default()
                    .push(ordinal);
            }
            for entity in &capsule.entities {
                index
                    .entities
                    .entry(stable_hash64(entity))
                    .or_default()
                    .push(ordinal);
            }
            index
                .semantic_buckets
                .entry(semantic_bucket(capsule.semantic_bits))
                .or_default()
                .push(ordinal);
            for edge in &capsule.relation_edges {
                if edge.kind == RelationKind::Supersedes {
                    index
                        .supersedes_by_target
                        .entry(edge.to)
                        .or_default()
                        .push(ordinal);
                }
            }
        }
        for values in index
            .anchors
            .values_mut()
            .chain(index.entities.values_mut())
            .chain(index.semantic_buckets.values_mut())
            .chain(index.supersedes_by_target.values_mut())
        {
            values.sort_unstable();
            values.dedup();
        }
        index
    }
}

pub fn stable_hash64(value: &str) -> u64 {
    let hash = blake3::hash(value.as_bytes());
    u64::from_le_bytes(hash.as_bytes()[0..8].try_into().expect("slice length"))
}

pub fn semantic_bucket(bits: [u64; 4]) -> u16 {
    (bits[0] & 0xffff) as u16
}
