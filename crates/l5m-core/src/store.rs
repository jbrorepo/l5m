use std::{
    cmp::Ordering,
    collections::HashMap,
    path::{Path, PathBuf},
    time::Instant,
};

use serde::{Deserialize, Serialize};

use crate::{
    compiler::parse_u128,
    frame::{CoverageReport, FrameCapsule},
    probe::MemoryProbe,
    scoring::{overlap_count, score_capsule},
    MemoryCapsule, MemoryFrame, Result, Segment,
};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum RetrievalMode {
    #[default]
    L5m,
    ParentAggregate,
    HybridBm25L5m,
    RrfFusionParent,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QueryRequest {
    pub query: String,
    pub tenant_id: u64,
    pub as_of: i64,
    pub context_mask: String,
    pub policy_mask: String,
    pub trust_floor: u8,
    pub max_capsules: usize,
    pub max_tokens: usize,
    #[serde(default)]
    pub include_supporting: bool,
    #[serde(default)]
    pub include_contradictions: bool,
    #[serde(default = "default_max_hops")]
    pub max_hops: u8,
    #[serde(default)]
    pub mode: RetrievalMode,
}

impl QueryRequest {
    pub fn to_probe(&self) -> Result<MemoryProbe> {
        let mut probe = MemoryProbe::build(
            &self.query,
            self.tenant_id,
            self.as_of,
            parse_u128(&self.context_mask)?,
            parse_u128(&self.policy_mask)?,
            self.trust_floor,
        );
        probe.max_capsules = self.max_capsules;
        probe.max_tokens = self.max_tokens;
        probe.include_supporting = self.include_supporting;
        probe.include_contradictions = self.include_contradictions;
        probe.max_hops = self.max_hops;
        Ok(probe)
    }

    pub fn config_hash(&self) -> [u8; 32] {
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        *blake3::hash(&bytes).as_bytes()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QueryResponse {
    pub frame: MemoryFrame,
    pub mode: RetrievalMode,
    pub config_hash: [u8; 32],
    pub segment_count: usize,
    pub segment_metadata: Vec<SegmentMetadata>,
    pub total_retrieval_ns: u128,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SegmentMetadata {
    pub path: PathBuf,
    pub epoch: u64,
    pub tenant_id: u64,
    pub capsule_count: usize,
}

pub struct MemoryStore {
    segments: Vec<LoadedSegment>,
}

struct LoadedSegment {
    path: PathBuf,
    segment: Segment,
}

impl MemoryStore {
    pub fn open_segments<I, P>(paths: I) -> Result<Self>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut segments = Vec::new();
        for path in paths {
            let path = path.as_ref().to_path_buf();
            segments.push(LoadedSegment {
                segment: Segment::open(&path)?,
                path,
            });
        }
        Ok(Self { segments })
    }

    pub fn query(&self, request: &QueryRequest) -> Result<QueryResponse> {
        let started = Instant::now();
        let probe = request.to_probe()?;
        let frame = match request.mode {
            RetrievalMode::L5m => self.retrieve_l5m(&probe)?,
            RetrievalMode::ParentAggregate => self.retrieve_parent_aggregate(&probe)?,
            RetrievalMode::HybridBm25L5m | RetrievalMode::RrfFusionParent => {
                self.retrieve_fusion(&probe, request.mode == RetrievalMode::RrfFusionParent)
            }
        };
        Ok(QueryResponse {
            frame,
            mode: request.mode,
            config_hash: request.config_hash(),
            segment_count: self.segments.len(),
            segment_metadata: self.segment_metadata(),
            total_retrieval_ns: started.elapsed().as_nanos(),
        })
    }

    fn retrieve_l5m(&self, probe: &MemoryProbe) -> Result<MemoryFrame> {
        let mut frames = Vec::new();
        for loaded in &self.segments {
            frames.push(crate::retrieve(&loaded.segment, probe)?);
        }
        Ok(merge_frames(frames, probe))
    }

    fn retrieve_parent_aggregate(&self, probe: &MemoryProbe) -> Result<MemoryFrame> {
        let mut wide = probe.clone();
        wide.max_capsules = probe.max_capsules.saturating_mul(4).max(probe.max_capsules);
        let mut frame = self.retrieve_l5m(&wide)?;
        frame.capsules = aggregate_frame_capsules(frame.capsules, probe.max_capsules);
        Ok(frame)
    }

    fn retrieve_fusion(&self, probe: &MemoryProbe, parent_aggregate: bool) -> MemoryFrame {
        let mut scored = Vec::new();
        let mut coverage = CoverageReport::default();
        let mut epoch = 0;

        for loaded in &self.segments {
            epoch = epoch.max(loaded.segment.epoch());
            for capsule in loaded.segment.capsules() {
                let hard_gates = hard_gates_pass(capsule, probe);
                coverage.context_valid_count +=
                    usize::from(capsule.context_mask & probe.context_mask != 0);
                coverage.temporal_valid_count += usize::from(
                    capsule.valid_from <= probe.as_of
                        && capsule.valid_until.is_none_or(|until| until >= probe.as_of),
                );
                coverage.trust_floor_met_count +=
                    usize::from(capsule.trust_level >= probe.trust_floor);
                if !hard_gates {
                    continue;
                }
                let lexical = lexical_score(capsule, probe);
                let semantic = score_capsule(&loaded.segment, capsule, probe);
                let exact = exact_score(capsule, probe);
                let temporal = temporal_score(capsule, probe);
                let score = if parent_aggregate {
                    rrf_like_score(lexical, semantic, exact, temporal)
                } else {
                    lexical * 1.2 + semantic + exact * 2.0 + temporal
                };
                scored.push(frame_capsule(capsule, score));
            }
        }

        coverage.candidate_count_before_scoring = scored.len();
        scored.sort_by(compare_frame_capsules);
        let capsules = if parent_aggregate {
            aggregate_frame_capsules(scored, probe.max_capsules)
        } else {
            scored.into_iter().take(probe.max_capsules).collect()
        };

        MemoryFrame {
            epoch,
            query_hash: *blake3::hash(probe.query_text.as_bytes()).as_bytes(),
            capsules,
            conflicts: Vec::new(),
            coverage,
        }
    }

    pub fn segment_metadata(&self) -> Vec<SegmentMetadata> {
        self.segments
            .iter()
            .map(|loaded| SegmentMetadata {
                path: loaded.path.clone(),
                epoch: loaded.segment.epoch(),
                tenant_id: loaded.segment.tenant_id(),
                capsule_count: loaded.segment.capsule_count(),
            })
            .collect()
    }
}

fn default_max_hops() -> u8 {
    1
}

fn merge_frames(mut frames: Vec<MemoryFrame>, probe: &MemoryProbe) -> MemoryFrame {
    let mut merged = MemoryFrame {
        epoch: 0,
        query_hash: *blake3::hash(probe.query_text.as_bytes()).as_bytes(),
        capsules: Vec::new(),
        conflicts: Vec::new(),
        coverage: CoverageReport::default(),
    };
    for frame in frames.drain(..) {
        merged.epoch = merged.epoch.max(frame.epoch);
        merged.capsules.extend(frame.capsules);
        merged.conflicts.extend(frame.conflicts);
        merged.coverage.exact_entity_match |= frame.coverage.exact_entity_match;
        merged.coverage.anchor_match_count += frame.coverage.anchor_match_count;
        merged.coverage.temporal_valid_count += frame.coverage.temporal_valid_count;
        merged.coverage.trust_floor_met_count += frame.coverage.trust_floor_met_count;
        merged.coverage.context_valid_count += frame.coverage.context_valid_count;
        merged.coverage.candidate_count_before_scoring +=
            frame.coverage.candidate_count_before_scoring;
    }
    merged.capsules.sort_by(compare_frame_capsules);
    merged.capsules.truncate(probe.max_capsules);
    merged
}

fn aggregate_frame_capsules(capsules: Vec<FrameCapsule>, limit: usize) -> Vec<FrameCapsule> {
    let mut grouped: HashMap<u64, FrameCapsule> = HashMap::new();
    let mut support_counts: HashMap<u64, usize> = HashMap::new();
    for capsule in capsules {
        let parent = capsule.source_id;
        let count = support_counts.entry(parent).or_default();
        *count += 1;
        grouped
            .entry(parent)
            .and_modify(|current| {
                if capsule.score > current.score {
                    *current = capsule.clone();
                }
            })
            .or_insert(capsule);
    }
    let mut aggregated = grouped.into_values().collect::<Vec<_>>();
    for capsule in &mut aggregated {
        let support = support_counts.get(&capsule.source_id).copied().unwrap_or(1);
        capsule.score += support.saturating_sub(1) as f32 * 0.03;
    }
    aggregated.sort_by(compare_frame_capsules);
    aggregated.truncate(limit);
    aggregated
}

fn hard_gates_pass(capsule: &MemoryCapsule, probe: &MemoryProbe) -> bool {
    capsule.tenant_id == probe.tenant_id
        && capsule.context_mask & probe.context_mask != 0
        && capsule.policy_mask & probe.caller_policy_mask != 0
        && capsule.trust_level >= probe.trust_floor
        && capsule.valid_from <= probe.as_of
        && capsule.valid_until.is_none_or(|until| until >= probe.as_of)
}

fn lexical_score(capsule: &MemoryCapsule, probe: &MemoryProbe) -> f32 {
    let text = format!("{} {}", capsule.claim, capsule.evidence).to_ascii_lowercase();
    let term_overlap = probe
        .anchors
        .iter()
        .filter(|term| text.contains(term.as_str()))
        .count() as f32;
    term_overlap + overlap_count(&capsule.anchors, &probe.anchors) as f32
}

fn exact_score(capsule: &MemoryCapsule, probe: &MemoryProbe) -> f32 {
    (overlap_count(&capsule.entities, &probe.entities)
        + overlap_count(&capsule.anchors, &probe.anchors)) as f32
}

fn temporal_score(capsule: &MemoryCapsule, probe: &MemoryProbe) -> f32 {
    let age = (probe.as_of - capsule.last_verified_at).max(0) as f32;
    1.0 / (1.0 + age / 86_400.0)
}

fn rrf_like_score(lexical: f32, semantic: f32, exact: f32, temporal: f32) -> f32 {
    lexical * 0.8 + semantic * 1.0 + exact * 1.5 + temporal * 0.5
}

fn frame_capsule(capsule: &MemoryCapsule, score: f32) -> FrameCapsule {
    FrameCapsule {
        capsule_id: capsule.capsule_id,
        claim: capsule.claim.clone(),
        evidence: capsule.evidence.clone(),
        trust_level: capsule.trust_level,
        valid_from: capsule.valid_from,
        valid_until: capsule.valid_until,
        source_id: capsule.source_id,
        source_hash: capsule.source_hash,
        relation_notes: Vec::new(),
        score,
    }
}

fn compare_frame_capsules(left: &FrameCapsule, right: &FrameCapsule) -> Ordering {
    right
        .score
        .partial_cmp(&left.score)
        .unwrap_or(Ordering::Equal)
        .then_with(|| right.trust_level.cmp(&left.trust_level))
        .then_with(|| right.valid_from.cmp(&left.valid_from))
}
