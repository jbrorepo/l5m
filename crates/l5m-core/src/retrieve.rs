use std::cmp::Ordering;

use crate::{
    bitset::BitSet,
    frame::{CoverageReport, FrameCapsule},
    index::{semantic_bucket, stable_hash64},
    scoring::{hamming_distance, overlap_count, score_capsule},
    MemoryCapsule, MemoryFrame, MemoryProbe, RelationKind, Result, Segment,
};

#[derive(Clone, Debug)]
pub struct RetrievalConfig {
    pub semantic_hamming_threshold: u32,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            semantic_hamming_threshold: 180,
        }
    }
}

pub fn retrieve(segment: &Segment, probe: &MemoryProbe) -> Result<MemoryFrame> {
    retrieve_with_config(segment, probe, &RetrievalConfig::default())
}

pub fn retrieve_with_config(
    segment: &Segment,
    probe: &MemoryProbe,
    config: &RetrievalConfig,
) -> Result<MemoryFrame> {
    let mut candidates = BitSet::new(segment.capsule_count());
    let mut coverage = CoverageReport::default();

    for (ordinal, capsule) in segment.capsules().iter().enumerate() {
        let context_ok = capsule.context_mask & probe.context_mask != 0;
        let temporal_ok = capsule.valid_from <= probe.as_of
            && capsule.valid_until.is_none_or(|until| until >= probe.as_of)
            && !is_superseded_by_visible_current(segment, capsule, probe);
        let trust_ok = capsule.trust_level >= probe.trust_floor;

        coverage.context_valid_count += usize::from(context_ok);
        coverage.temporal_valid_count += usize::from(temporal_ok);
        coverage.trust_floor_met_count += usize::from(trust_ok);

        if capsule.tenant_id == probe.tenant_id
            && context_ok
            && capsule.policy_mask & probe.caller_policy_mask != 0
            && temporal_ok
            && trust_ok
        {
            candidates.set(ordinal);
        }
    }

    let mut lookup = BitSet::new(segment.capsule_count());
    let mut anchor_match_count = 0usize;
    let mut exact_entity_match = false;

    for entity in &probe.entities {
        if let Some(ordinals) = segment.index().entities.get(&stable_hash64(entity)) {
            for ordinal in ordinals {
                if candidates.get(*ordinal) {
                    exact_entity_match = true;
                    lookup.set(*ordinal);
                }
            }
        }
    }
    for anchor in &probe.anchors {
        if let Some(ordinals) = segment.index().anchors.get(&stable_hash64(anchor)) {
            for ordinal in ordinals {
                if candidates.get(*ordinal) {
                    anchor_match_count += 1;
                    lookup.set(*ordinal);
                }
            }
        }
    }

    let mut semantic = BitSet::new(segment.capsule_count());
    if let Some(ordinals) = segment
        .index()
        .semantic_buckets
        .get(&semantic_bucket(probe.semantic_bits))
    {
        for ordinal in ordinals {
            if candidates.get(*ordinal) {
                semantic.set(*ordinal);
            }
        }
    }
    for ordinal in candidates.iter_ones() {
        let Some(capsule) = segment.capsule(ordinal) else {
            continue;
        };
        if hamming_distance(probe.semantic_bits, capsule.semantic_bits)
            <= config.semantic_hamming_threshold
        {
            semantic.set(ordinal);
        }
    }

    let mut narrowed = lookup.clone();
    narrowed.or_assign(&semantic);
    if narrowed.count_ones() > 0 {
        candidates.and_assign(&narrowed);
    }

    coverage.exact_entity_match = exact_entity_match;
    coverage.anchor_match_count = anchor_match_count;
    coverage.candidate_count_before_scoring = candidates.count_ones();

    let mut scored = candidates
        .iter_ones()
        .filter_map(|ordinal| segment.capsule(ordinal))
        .map(|capsule| {
            let mut score = score_capsule(segment, capsule, probe);
            score += overlap_count(&capsule.entities, &probe.entities) as f32 * 2.0;
            (capsule, score)
        })
        .collect::<Vec<_>>();

    scored.sort_by(|(left, left_score), (right, right_score)| {
        right_score
            .partial_cmp(left_score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| right.trust_level.cmp(&left.trust_level))
            .then_with(|| right.last_verified_at.cmp(&left.last_verified_at))
    });

    let mut token_budget = probe.max_tokens;
    let mut capsules = Vec::new();
    for (capsule, score) in scored.into_iter().take(probe.max_capsules) {
        let approx_tokens =
            approximate_tokens(&capsule.claim) + approximate_tokens(&capsule.evidence);
        if capsules.is_empty() || approx_tokens <= token_budget {
            token_budget = token_budget.saturating_sub(approx_tokens);
            capsules.push(frame_capsule(capsule, score, Vec::new()));
        }
    }

    let conflicts = expand_relations(segment, probe, &capsules);

    Ok(MemoryFrame {
        epoch: segment.epoch(),
        query_hash: *blake3::hash(probe.query_text.as_bytes()).as_bytes(),
        capsules,
        conflicts,
        coverage,
    })
}

fn is_superseded_by_visible_current(
    segment: &Segment,
    capsule: &MemoryCapsule,
    probe: &MemoryProbe,
) -> bool {
    segment
        .index()
        .supersedes_by_target
        .get(&capsule.capsule_id)
        .is_some_and(|ordinals| {
            ordinals.iter().any(|ordinal| {
                segment
                    .capsule(*ordinal)
                    .is_some_and(|other| passes_hard_gates(other, probe))
            })
        })
}

fn expand_relations(
    segment: &Segment,
    probe: &MemoryProbe,
    selected: &[FrameCapsule],
) -> Vec<FrameCapsule> {
    if probe.max_hops == 0 || (!probe.include_supporting && !probe.include_contradictions) {
        return Vec::new();
    }
    let mut out: Vec<FrameCapsule> = Vec::new();
    for selected_capsule in selected {
        let Some(source) = segment.capsule_by_id(selected_capsule.capsule_id) else {
            continue;
        };
        for edge in &source.relation_edges {
            let include = match edge.kind {
                RelationKind::Supports => probe.include_supporting,
                RelationKind::Contradicts | RelationKind::Supersedes => {
                    probe.include_contradictions
                }
                _ => false,
            };
            if !include {
                continue;
            }
            let Some(related) = segment.capsule_by_id(edge.to) else {
                continue;
            };
            let allow_metadata_exception = matches!(
                edge.kind,
                RelationKind::Contradicts | RelationKind::Supersedes
            ) && probe.include_contradictions;
            if passes_hard_gates(related, probe)
                || (allow_metadata_exception && passes_authorization_gates(related, probe))
            {
                let note = format!(
                    "{} from {} to {} weight {}",
                    edge.kind.as_str(),
                    edge.from,
                    edge.to,
                    edge.weight
                );
                let mut merged = false;
                for existing in &mut out {
                    if existing.capsule_id == related.capsule_id {
                        existing.relation_notes.push(note.clone());
                        merged = true;
                        break;
                    }
                }
                if !merged {
                    out.push(frame_capsule(related, 0.0, vec![note]));
                }
            }
        }
    }
    out
}

fn passes_hard_gates(capsule: &MemoryCapsule, probe: &MemoryProbe) -> bool {
    passes_authorization_gates(capsule, probe)
        && capsule.valid_from <= probe.as_of
        && capsule.valid_until.is_none_or(|until| until >= probe.as_of)
}

fn passes_authorization_gates(capsule: &MemoryCapsule, probe: &MemoryProbe) -> bool {
    capsule.tenant_id == probe.tenant_id
        && capsule.context_mask & probe.context_mask != 0
        && capsule.policy_mask & probe.caller_policy_mask != 0
        && capsule.trust_level >= probe.trust_floor
}

fn frame_capsule(capsule: &MemoryCapsule, score: f32, relation_notes: Vec<String>) -> FrameCapsule {
    FrameCapsule {
        capsule_id: capsule.capsule_id,
        claim: capsule.claim.clone(),
        evidence: capsule.evidence.clone(),
        trust_level: capsule.trust_level,
        valid_from: capsule.valid_from,
        valid_until: capsule.valid_until,
        source_id: capsule.source_id,
        source_hash: capsule.source_hash,
        relation_notes,
        score,
    }
}

fn approximate_tokens(text: &str) -> usize {
    text.split_whitespace().count().max(1)
}
