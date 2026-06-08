use std::cmp::Ordering;
use std::time::Instant;

use crate::{
    bitset::BitSet,
    frame::{CoverageReport, FrameCapsule},
    index::{semantic_bucket, stable_hash64},
    scoring::{cosine_similarity, hamming_distance, overlap_count, score_capsule},
    MemoryCapsule, MemoryFrame, MemoryProbe, RelationKind, Result, Segment,
};

#[derive(Clone, Debug)]
pub struct RetrievalConfig {
    pub semantic_hamming_threshold: u32,
    /// Upper bound on how many authorized candidates are fully scored. Gates
    /// always run first, so this never widens access; it only caps the dominant
    /// scoring cost at scale by keeping exact lookup matches plus the closest
    /// (lowest-hamming) candidates. Small candidate sets are unaffected.
    pub max_scored_candidates: usize,
    /// When the authorized (gated) candidate set is larger than this, use the
    /// sublinear LSH index for semantic candidate generation instead of the
    /// exact O(N) hamming scan. Below it, the exact path runs, so small
    /// candidate sets (e.g. the public benchmarks) get byte-identical results.
    pub ann_candidate_threshold: usize,
    /// Reciprocal-rank-fusion constant for hybrid (lexical ⊕ dense) ranking.
    /// Only used when the probe carries a dense embedding.
    pub embed_rrf_k: f64,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            semantic_hamming_threshold: 180,
            max_scored_candidates: 2048,
            ann_candidate_threshold: 4096,
            embed_rrf_k: 60.0,
        }
    }
}

/// Per-phase wall-clock breakdown of a single retrieval, for profiling the hot
/// path. Populated by [`retrieve_with_timings`]; zero-cost for callers that use
/// [`retrieve`]/[`retrieve_with_config`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RetrievalTimings {
    /// Linear gate scan (tenant/context/policy/temporal/trust).
    pub gate_filter_ns: u64,
    /// Anchor/entity/semantic candidate narrowing.
    pub lookup_ns: u64,
    /// Scoring + sorting of the candidate set.
    pub scoring_ns: u64,
    /// Relation (support/contradiction) expansion.
    pub relation_ns: u64,
    pub total_ns: u64,
}

pub fn retrieve(segment: &Segment, probe: &MemoryProbe) -> Result<MemoryFrame> {
    retrieve_with_config(segment, probe, &RetrievalConfig::default())
}

pub fn retrieve_with_config(
    segment: &Segment,
    probe: &MemoryProbe,
    config: &RetrievalConfig,
) -> Result<MemoryFrame> {
    Ok(retrieve_with_timings(segment, probe, config)?.0)
}

/// Like [`retrieve_with_config`] but also returns a per-phase timing breakdown.
pub fn retrieve_with_timings(
    segment: &Segment,
    probe: &MemoryProbe,
    config: &RetrievalConfig,
) -> Result<(MemoryFrame, RetrievalTimings)> {
    let mut timings = RetrievalTimings::default();
    let total_start = Instant::now();
    let mut candidates = BitSet::new(segment.capsule_count());
    let mut coverage = CoverageReport::default();

    let gate_start = Instant::now();
    let index = segment.index();
    // The tenant gate is the security boundary and the first selectivity win:
    // scan only this tenant's ordinals (sublinear in total segment size), then
    // apply the remaining gates against the compact columnar rows.
    if let Some(tenant_ordinals) = index.tenant_postings.get(&probe.tenant_id) {
        for &ord32 in tenant_ordinals {
            let ordinal = ord32 as usize;
            let row = &index.gate_rows[ordinal];
            let context_ok = row.context_mask & probe.context_mask != 0;
            let window_ok = row.valid_from <= probe.as_of
                && row.valid_until.is_none_or(|until| until >= probe.as_of);
            // Supersession is probe-dependent and only possible for capsules
            // that are actually the target of a supersede edge — skip the check
            // (and the capsule fetch) for everything else.
            let temporal_ok = window_ok
                && !(index.supersede_target[ordinal]
                    && segment.capsule(ordinal).is_some_and(|capsule| {
                        is_superseded_by_visible_current(segment, capsule, probe)
                    }));
            let trust_ok = row.trust_level >= probe.trust_floor;

            coverage.context_valid_count += usize::from(context_ok);
            coverage.temporal_valid_count += usize::from(temporal_ok);
            coverage.trust_floor_met_count += usize::from(trust_ok);

            if context_ok
                && row.policy_mask & probe.caller_policy_mask != 0
                && temporal_ok
                && trust_ok
            {
                candidates.set(ordinal);
            }
        }
    }
    timings.gate_filter_ns = gate_start.elapsed().as_nanos() as u64;

    let lookup_start = Instant::now();
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

    // Candidate generation. Above the threshold the gated set is large enough
    // that the exact O(N) hamming scan dominates, so we switch to sublinear LSH
    // candidate generation. Both paths only ever consider capsules already in
    // the authorized `candidates` set, so the security guarantee is identical.
    let gated_count = candidates.count_ones();
    let scored_ordinals: Vec<usize> = if gated_count > config.ann_candidate_threshold {
        let mut pool = ann_candidate_pool(segment, probe, &candidates, &lookup, config);
        if pool.is_empty() {
            // Pathological probe with no band/lookup hits: fall back to exact so
            // we never silently return nothing where a scan would have found a hit.
            pool = exact_scored_ordinals(segment, probe, &mut candidates, &lookup, config);
        }
        pool
    } else {
        exact_scored_ordinals(segment, probe, &mut candidates, &lookup, config)
    };

    coverage.exact_entity_match = exact_entity_match;
    coverage.anchor_match_count = anchor_match_count;
    coverage.candidate_count_before_scoring = scored_ordinals.len();
    timings.lookup_ns = lookup_start.elapsed().as_nanos() as u64;

    let scoring_start = Instant::now();
    let mut scored = scored_ordinals
        .iter()
        .filter_map(|ordinal| segment.capsule(*ordinal))
        .map(|capsule| {
            let mut score = score_capsule(segment, capsule, probe);
            score += overlap_count(&capsule.entities, &probe.entities) as f32 * 2.0;
            (capsule, score)
        })
        .collect::<Vec<_>>();

    // Hybrid (lexical ⊕ dense) fusion: when the probe carries a dense embedding,
    // re-rank the candidate pool by reciprocal-rank fusion of the lexical score
    // and the dense cosine similarity. Runs only on the already-gated pool, so
    // the security guarantee is unaffected; no-op when no embedding is present
    // (ranking stays byte-identical to pure lexical).
    if !probe.embedding.is_empty() && !scored.is_empty() {
        let count = scored.len();
        let dense: Vec<f32> = scored
            .iter()
            .map(|(capsule, _)| cosine_similarity(&probe.embedding, &capsule.embedding))
            .collect();
        let mut lexical_order: Vec<usize> = (0..count).collect();
        lexical_order.sort_by(|&a, &b| {
            scored[b]
                .1
                .partial_cmp(&scored[a].1)
                .unwrap_or(Ordering::Equal)
                .then_with(|| a.cmp(&b))
        });
        let mut dense_order: Vec<usize> = (0..count).collect();
        dense_order.sort_by(|&a, &b| {
            dense[b]
                .partial_cmp(&dense[a])
                .unwrap_or(Ordering::Equal)
                .then_with(|| a.cmp(&b))
        });
        let k = config.embed_rrf_k;
        let mut fused = vec![0.0f64; count];
        for (rank, &index) in lexical_order.iter().enumerate() {
            fused[index] += 1.0 / (k + rank as f64 + 1.0);
        }
        for (rank, &index) in dense_order.iter().enumerate() {
            fused[index] += 1.0 / (k + rank as f64 + 1.0);
        }
        for (index, entry) in scored.iter_mut().enumerate() {
            entry.1 = fused[index] as f32;
        }
    }

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

    timings.scoring_ns = scoring_start.elapsed().as_nanos() as u64;

    let relation_start = Instant::now();
    let conflicts = expand_relations(segment, probe, &capsules);
    timings.relation_ns = relation_start.elapsed().as_nanos() as u64;
    timings.total_ns = total_start.elapsed().as_nanos() as u64;

    Ok((
        MemoryFrame {
            epoch: segment.epoch(),
            query_hash: *blake3::hash(probe.query_text.as_bytes()).as_bytes(),
            capsules,
            conflicts,
            coverage,
        },
        timings,
    ))
}

/// Exact candidate generation: semantic-bucket prefilter + full hamming scan
/// over the gated set + lowest-hamming cap. Identical to the pre-LSH behavior;
/// used for small candidate sets and as the ANN fallback.
fn exact_scored_ordinals(
    segment: &Segment,
    probe: &MemoryProbe,
    candidates: &mut BitSet,
    lookup: &BitSet,
    config: &RetrievalConfig,
) -> Vec<usize> {
    let index = segment.index();
    let fingerprints = &index.fingerprints;
    let mut semantic = BitSet::new(segment.capsule_count());
    if let Some(ordinals) = index
        .semantic_buckets
        .get(&semantic_bucket(probe.semantic_bits))
    {
        for ordinal in ordinals {
            if candidates.get(*ordinal) {
                semantic.set(*ordinal);
            }
        }
    }
    let mut hamming_by_ordinal: Vec<(usize, u32)> = Vec::with_capacity(candidates.count_ones());
    for ordinal in candidates.iter_ones() {
        let distance = hamming_distance(probe.semantic_bits, fingerprints[ordinal]);
        hamming_by_ordinal.push((ordinal, distance));
        if distance <= config.semantic_hamming_threshold {
            semantic.set(ordinal);
        }
    }
    let mut narrowed = lookup.clone();
    narrowed.or_assign(&semantic);
    if narrowed.count_ones() > 0 {
        candidates.and_assign(&narrowed);
    }
    if candidates.count_ones() > config.max_scored_candidates {
        let mut survivors: Vec<(usize, u32)> = hamming_by_ordinal
            .into_iter()
            .filter(|(ordinal, _)| candidates.get(*ordinal))
            .collect();
        survivors.sort_by_key(|(ordinal, distance)| (!lookup.get(*ordinal), *distance, *ordinal));
        survivors.truncate(config.max_scored_candidates);
        let mut ordinals: Vec<usize> = survivors.into_iter().map(|(ordinal, _)| ordinal).collect();
        ordinals.sort_unstable();
        ordinals
    } else {
        candidates.iter_ones().collect()
    }
}

/// Sublinear candidate generation via the LSH index: visit only the postings
/// that share a band value with the probe, intersected with the authorized gate
/// set, plus exact entity/anchor matches. Rank by (exact-match, hamming), cap.
fn ann_candidate_pool(
    segment: &Segment,
    probe: &MemoryProbe,
    candidates: &BitSet,
    lookup: &BitSet,
    config: &RetrievalConfig,
) -> Vec<usize> {
    let index = segment.index();
    let fingerprints = &index.fingerprints;
    let mut seen = BitSet::new(segment.capsule_count());
    let mut pool: Vec<(usize, u32)> = Vec::new();
    for ordinal in lookup.iter_ones() {
        if candidates.get(ordinal) && !seen.get(ordinal) {
            seen.set(ordinal);
            pool.push((
                ordinal,
                hamming_distance(probe.semantic_bits, fingerprints[ordinal]),
            ));
        }
    }
    index
        .lsh
        .for_each_candidate(&probe.semantic_bits, |ordinal| {
            if candidates.get(ordinal) && !seen.get(ordinal) {
                seen.set(ordinal);
                pool.push((
                    ordinal,
                    hamming_distance(probe.semantic_bits, fingerprints[ordinal]),
                ));
            }
        });
    pool.sort_by_key(|(ordinal, distance)| (!lookup.get(*ordinal), *distance, *ordinal));
    pool.truncate(config.max_scored_candidates);
    let mut ordinals: Vec<usize> = pool.into_iter().map(|(ordinal, _)| ordinal).collect();
    ordinals.sort_unstable();
    ordinals
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
