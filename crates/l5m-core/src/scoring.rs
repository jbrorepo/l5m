use crate::{MemoryCapsule, MemoryProbe, Segment};

pub fn hamming_distance(left: [u64; 4], right: [u64; 4]) -> u32 {
    left.iter()
        .zip(right)
        .map(|(a, b)| (a ^ b).count_ones())
        .sum()
}

pub fn residual_dot(left: &[i8; 64], right: &[i8; 64]) -> i32 {
    left.iter()
        .zip(right)
        .map(|(a, b)| i32::from(*a) * i32::from(*b))
        .sum()
}

pub fn score_capsule(segment: &Segment, capsule: &MemoryCapsule, probe: &MemoryProbe) -> f32 {
    let entity_overlap = overlap_count(&capsule.entities, &probe.entities) as f32;
    let anchor_overlap = overlap_count(&capsule.anchors, &probe.anchors) as f32;
    let hamming = hamming_distance(probe.semantic_bits, capsule.semantic_bits);
    let hamming_score = 1.0 - (hamming as f32 / 256.0);
    let dot = residual_dot(&probe.residual, &capsule.residual) as f32 / 128.0;
    let context_specificity = capsule.context_mask.count_ones() as f32 / 128.0;
    let trust = capsule.trust_level as f32 / 10.0;
    let age_days = ((probe.as_of - capsule.last_verified_at).max(0) as f32) / 86_400.0;
    let freshness = 1.0 / (1.0 + age_days / 365.0);
    let support = segment
        .relations_from(capsule.capsule_id)
        .iter()
        .filter(|edge| matches!(edge.kind, crate::RelationKind::Supports))
        .count() as f32
        * 0.05;
    let poison_penalty = f32::from(capsule.poison_risk) * 0.5;

    entity_overlap * 4.0
        + anchor_overlap * 1.2
        + hamming_score * 2.0
        + dot
        + context_specificity
        + trust
        + freshness
        + support
        - poison_penalty
}

pub fn overlap_count(left: &[String], right: &[String]) -> usize {
    left.iter()
        .filter(|value| right.iter().any(|candidate| candidate == *value))
        .count()
}
