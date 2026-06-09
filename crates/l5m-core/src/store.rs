use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
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
    /// Optional dense query embedding (computed offline) enabling hybrid
    /// lexical ⊕ dense ranking.
    #[serde(default)]
    pub embedding: Vec<f32>,
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
        probe.embedding = self.embedding.clone();
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

/// Default size at which the active write buffer seals into an immutable run.
/// Bounding the buffer is what makes a write amortized O(1): each write rebuilds
/// only the (bounded) active segment, never the whole delta.
pub const DEFAULT_DELTA_SEAL_THRESHOLD: usize = 1024;

/// Default number of sealed runs that triggers an automatic minor compaction,
/// bounding per-query segment fan-out on a long-running store.
pub const DEFAULT_AUTO_COMPACT_RUNS: usize = 16;

pub struct MemoryStore {
    segments: Vec<LoadedSegment>,
    /// Active, in-RAM write buffer of the most recent ingests/updates. Bounded by
    /// `seal_threshold`; only this (small) buffer is re-indexed per write.
    delta: Vec<MemoryCapsule>,
    /// In-memory segment built from the active `delta` buffer so live writes share
    /// the exact gate/index/retrieval path as compiled base segments. Rebuilt on
    /// mutation — but the buffer is bounded, so the rebuild is O(seal_threshold).
    delta_segment: Option<Segment>,
    /// Immutable, already-indexed delta runs sealed from the active buffer once it
    /// filled (LSM-style). Queried alongside the base + active buffer; never
    /// rebuilt — folded away by compaction.
    sealed_deltas: Vec<Segment>,
    /// Seal the active buffer into a `sealed_deltas` run once it reaches this size.
    seal_threshold: usize,
    /// When `Some(n)`, automatically run a minor compaction once `n` sealed runs
    /// have accumulated, keeping per-query segment fan-out bounded. `None`
    /// disables automatic compaction (callers drive `compact`/`compact_delta`).
    auto_compact_runs: Option<usize>,
    /// Capsule ids hidden from results (deletes / supersessions).
    tombstones: HashSet<u128>,
    next_epoch: u64,
    metrics: crate::metrics::Metrics,
    /// When set, every mutation is durably appended here before it is
    /// acknowledged, so writes survive a crash/restart.
    wal: Option<crate::wal::Wal>,
}

struct LoadedSegment {
    path: Option<PathBuf>,
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
                path: Some(path),
            });
        }
        let next_epoch = segments
            .iter()
            .map(|loaded| loaded.segment.epoch())
            .max()
            .unwrap_or(0)
            + 1;
        Ok(Self {
            segments,
            delta: Vec::new(),
            delta_segment: None,
            sealed_deltas: Vec::new(),
            seal_threshold: DEFAULT_DELTA_SEAL_THRESHOLD,
            auto_compact_runs: Some(DEFAULT_AUTO_COMPACT_RUNS),
            tombstones: HashSet::new(),
            next_epoch,
            metrics: crate::metrics::Metrics::new(),
            wal: None,
        })
    }

    /// Start an empty store (no base segments) — useful for a pure real-time
    /// memory that begins from writes.
    pub fn empty() -> Self {
        Self {
            segments: Vec::new(),
            delta: Vec::new(),
            delta_segment: None,
            sealed_deltas: Vec::new(),
            seal_threshold: DEFAULT_DELTA_SEAL_THRESHOLD,
            auto_compact_runs: Some(DEFAULT_AUTO_COMPACT_RUNS),
            tombstones: HashSet::new(),
            next_epoch: 1,
            metrics: crate::metrics::Metrics::new(),
            wal: None,
        }
    }

    /// Override the active-buffer seal threshold (mainly for tests/tuning). A
    /// smaller threshold seals more often (more runs, cheaper individual writes);
    /// a larger one seals rarely (fewer runs, larger per-seal cost).
    pub fn with_seal_threshold(mut self, threshold: usize) -> Self {
        self.seal_threshold = threshold.max(1);
        self
    }

    /// Configure automatic minor compaction. `Some(n)` consolidates the delta
    /// once `n` sealed runs accumulate (bounding query fan-out); `None` disables
    /// it so the caller drives `compact`/`compact_delta` explicitly.
    pub fn with_auto_compaction(mut self, runs: Option<usize>) -> Self {
        self.auto_compact_runs = runs.map(|n| n.max(1));
        self
    }

    /// Open a **durable** store: load base segments, replay the write-ahead log
    /// to recover acknowledged writes, then keep the WAL open so future
    /// mutations are persisted before they return.
    pub fn open_durable<I, P>(segment_paths: I, wal_path: impl AsRef<Path>) -> Result<Self>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut store = Self::open_segments(segment_paths)?;
        // Replay with the WAL still detached so replay does not re-log.
        //
        // Fold the whole log into the final delta state in a single pass, then
        // build the delta index ONCE. Replaying op-by-op through `insert`/`delete`
        // would rebuild the delta segment on every entry — O(N^2) in the log size,
        // which dominates restart time on a busy store. Last-writer-wins per id is
        // preserved by applying ops in order into a positional map.
        let mut by_id: HashMap<u128, usize> = HashMap::new();
        let mut delta: Vec<MemoryCapsule> = Vec::new();
        let mut tombstones: HashSet<u128> = HashSet::new();
        let (mut inserts, mut deletes) = (0u64, 0u64);
        for op in crate::wal::Wal::replay(&wal_path)? {
            match op {
                crate::wal::WalOp::Insert { capsule } => {
                    let capsule = crate::compiler::capsule_from_json(&capsule)?;
                    let id = capsule.capsule_id;
                    tombstones.remove(&id);
                    match by_id.get(&id) {
                        Some(&pos) => delta[pos] = capsule,
                        None => {
                            by_id.insert(id, delta.len());
                            delta.push(capsule);
                        }
                    }
                    inserts += 1;
                }
                crate::wal::WalOp::Delete { id } => {
                    let id = crate::compiler::parse_u128(&id)?;
                    // Tombstone hides base-segment capsules too; drop any live delta
                    // version by orphaning its slot (removed from the canonical map).
                    by_id.remove(&id);
                    tombstones.insert(id);
                    deletes += 1;
                }
            }
        }
        // Keep only canonical slots: a slot survives iff `by_id` still points at it.
        // Deleted ids were removed from the map; a reinserted id points at its newer
        // slot, orphaning the older one.
        let mut live_delta = Vec::with_capacity(by_id.len());
        for (pos, capsule) in delta.into_iter().enumerate() {
            if by_id.get(&capsule.capsule_id) == Some(&pos) {
                live_delta.push(capsule);
            }
        }
        store.delta = live_delta;
        store.tombstones = tombstones;
        for _ in 0..inserts {
            store.metrics.record_insert();
        }
        for _ in 0..deletes {
            store.metrics.record_delete();
        }
        // Recovered state lands in the active buffer as a single run; the next
        // live write seals it if it is over threshold.
        store.rebuild_active_segment()?;
        store.wal = Some(crate::wal::Wal::open(&wal_path)?);
        Ok(store)
    }

    fn wal_append(&mut self, op: &crate::wal::WalOp) -> Result<()> {
        if let Some(wal) = self.wal.as_mut() {
            wal.append(op)?;
        }
        Ok(())
    }

    /// Operational metrics in Prometheus text format (also via
    /// `self.metrics().render_prometheus()`).
    pub fn metrics(&self) -> &crate::metrics::Metrics {
        &self.metrics
    }

    /// Ingest or update a memory at runtime. Same id replaces the prior live
    /// version; the capsule passes the identical gates at query time.
    pub fn insert(&mut self, capsule: MemoryCapsule) -> Result<()> {
        // Durably log before applying, so an acknowledged write is recoverable.
        let op = crate::wal::WalOp::Insert {
            capsule: crate::compiler::capsule_to_json(&capsule),
        };
        self.wal_append(&op)?;
        let tenant = capsule.tenant_id;
        self.push_into_buffer(capsule);
        self.metrics.record_insert();
        self.metrics.record_insert_for(tenant);
        self.seal_or_reindex()?;
        self.maybe_auto_compact()
    }

    /// Ingest from the same JSON shape `compile_segment` accepts.
    pub fn insert_json(&mut self, value: &serde_json::Value) -> Result<()> {
        self.insert(crate::compiler::capsule_from_json(value)?)
    }

    /// Batch ingest, re-indexing the active buffer once at the end (sealing
    /// full runs along the way). Prefer this for bulk writes.
    pub fn insert_many<I>(&mut self, capsules: I) -> Result<()>
    where
        I: IntoIterator<Item = MemoryCapsule>,
    {
        for capsule in capsules {
            let op = crate::wal::WalOp::Insert {
                capsule: crate::compiler::capsule_to_json(&capsule),
            };
            self.wal_append(&op)?;
            let tenant = capsule.tenant_id;
            self.push_into_buffer(capsule);
            self.metrics.record_insert();
            self.metrics.record_insert_for(tenant);
            if self.delta.len() >= self.seal_threshold {
                self.seal_active_buffer()?;
                self.maybe_auto_compact()?;
            }
        }
        self.rebuild_active_segment()
    }

    /// Batch ingest from JSON, rebuilding the delta index once. Prefer this over
    /// repeated `insert_json` for bulk loads.
    pub fn insert_many_json<'a, I>(&mut self, values: I) -> Result<()>
    where
        I: IntoIterator<Item = &'a serde_json::Value>,
    {
        let capsules = values
            .into_iter()
            .map(crate::compiler::capsule_from_json)
            .collect::<Result<Vec<_>>>()?;
        self.insert_many(capsules)
    }

    /// Hide a capsule id from all future results (delete / supersede). The
    /// tombstone masks the id across the base, every sealed run, and the active
    /// buffer at query time; we also drop any copy living in the active buffer.
    pub fn delete(&mut self, capsule_id: u128) -> Result<()> {
        self.wal_append(&crate::wal::WalOp::Delete {
            id: capsule_id.to_string(),
        })?;
        self.delta
            .retain(|existing| existing.capsule_id != capsule_id);
        self.tombstones.insert(capsule_id);
        self.metrics.record_delete();
        self.rebuild_active_segment()
    }

    /// Number of capsules in the active (unsealed) write buffer.
    pub fn delta_len(&self) -> usize {
        self.delta.len()
    }

    /// Number of sealed, immutable delta runs awaiting compaction.
    pub fn sealed_run_count(&self) -> usize {
        self.sealed_deltas.len()
    }

    /// Append into the active buffer, clearing any tombstone and superseding any
    /// older copy of the same id that is still in the buffer.
    fn push_into_buffer(&mut self, capsule: MemoryCapsule) {
        let id = capsule.capsule_id;
        self.tombstones.remove(&id);
        self.delta.retain(|existing| existing.capsule_id != id);
        self.delta.push(capsule);
    }

    /// After a single write: seal the buffer if it has filled, otherwise just
    /// re-index the (bounded) active buffer.
    fn seal_or_reindex(&mut self) -> Result<()> {
        if self.delta.len() >= self.seal_threshold {
            self.seal_active_buffer()
        } else {
            self.rebuild_active_segment()
        }
    }

    /// Re-index the active buffer into its in-RAM segment. O(buffer) — and the
    /// buffer is bounded by `seal_threshold`, so this is the per-write cost.
    fn rebuild_active_segment(&mut self) -> Result<()> {
        self.delta_segment = if self.delta.is_empty() {
            None
        } else {
            Some(Segment::from_capsules(self.delta.clone(), self.next_epoch)?)
        };
        Ok(())
    }

    /// Freeze the active buffer into an immutable sealed run and start a fresh
    /// buffer. The run is indexed once and never rebuilt until compaction.
    fn seal_active_buffer(&mut self) -> Result<()> {
        if self.delta.is_empty() {
            return Ok(());
        }
        let run = Segment::from_capsules(std::mem::take(&mut self.delta), self.next_epoch)?;
        self.next_epoch += 1;
        self.sealed_deltas.push(run);
        self.delta_segment = None;
        Ok(())
    }

    /// Collect live capsules (newest version per id wins, tombstoned ids dropped)
    /// across the active buffer and sealed runs, and — when `include_base` — the
    /// base segments. Recency order: active buffer → sealed runs newest-first →
    /// base, so the freshest copy of any id is the one kept.
    fn collect_live(&self, include_base: bool) -> Vec<MemoryCapsule> {
        let mut live: Vec<MemoryCapsule> = Vec::new();
        let mut seen: HashSet<u128> = HashSet::new();
        let mut take = |capsule: &MemoryCapsule| {
            if !self.tombstones.contains(&capsule.capsule_id) && seen.insert(capsule.capsule_id) {
                live.push(capsule.clone());
            }
        };
        for capsule in &self.delta {
            take(capsule);
        }
        for run in self.sealed_deltas.iter().rev() {
            for capsule in run.capsules() {
                take(capsule);
            }
        }
        if include_base {
            for loaded in &self.segments {
                for capsule in loaded.segment.capsules() {
                    take(capsule);
                }
            }
        }
        live
    }

    /// Reset all delta state (active buffer, sealed runs, tombstones) after the
    /// live set has been folded into a base segment.
    fn clear_delta_state(&mut self) {
        self.delta.clear();
        self.sealed_deltas.clear();
        self.delta_segment = None;
        self.tombstones.clear();
    }

    /// Fold the delta (active buffer + sealed runs) and base segments into a
    /// single fresh in-memory base segment (dropping tombstoned ids, newest
    /// version winning on id collisions) and clear the delta. Keeps the delta
    /// small (LSM-style).
    pub fn compact(&mut self) -> Result<()> {
        let live = self.collect_live(true);
        let compacted = Segment::from_capsules(live, self.next_epoch)?;
        self.segments = vec![LoadedSegment {
            path: None,
            segment: compacted,
        }];
        self.clear_delta_state();
        self.next_epoch += 1;
        Ok(())
    }

    /// Durable compaction: persist the live state to `checkpoint` on disk, make
    /// it the sole base segment, then truncate the WAL. The checkpoint is written
    /// (and fsync'd) **before** the WAL is truncated, so a crash mid-compaction
    /// recovers by replaying the WAL on top of the previous state — never losing
    /// an acknowledged write. After this, reopen with
    /// `open_durable([checkpoint], wal_path)`.
    pub fn compact_to(&mut self, checkpoint: impl AsRef<Path>) -> Result<()> {
        let checkpoint = checkpoint.as_ref();
        let live = self.collect_live(true);
        // 1) Durably write the checkpoint segment.
        crate::compiler::compile_capsules(checkpoint, live, self.next_epoch)?;
        // 2) Adopt it as the sole base.
        self.segments = vec![LoadedSegment {
            path: Some(checkpoint.to_path_buf()),
            segment: Segment::open(checkpoint)?,
        }];
        self.clear_delta_state();
        self.next_epoch += 1;
        // 3) Only now is it safe to drop the WAL history.
        if let Some(wal) = self.wal.as_mut() {
            wal.truncate()?;
        }
        Ok(())
    }

    /// Minor compaction: merge the active buffer + all sealed runs into a single
    /// sealed run, **without touching the base**. This bounds per-query segment
    /// fan-out (the cost that grows as runs accumulate) at O(delta) cost rather
    /// than the O(base + delta) of a full `compact`. Tombstones are retained so
    /// base copies of deleted ids stay hidden; the base is untouched, so the
    /// merged run stays "newer than base" and still wins on id collisions.
    pub fn compact_delta(&mut self) -> Result<()> {
        // Nothing to consolidate unless there are at least two delta segments
        // (sealed runs and/or the active buffer) to merge.
        if self.sealed_deltas.is_empty() {
            return Ok(());
        }
        let live = self.collect_live(false);
        self.delta.clear();
        self.delta_segment = None;
        self.sealed_deltas = if live.is_empty() {
            Vec::new()
        } else {
            vec![Segment::from_capsules(live, self.next_epoch)?]
        };
        self.next_epoch += 1;
        Ok(())
    }

    /// If automatic compaction is enabled and the sealed-run count has reached the
    /// configured threshold, run a minor compaction to bound query fan-out.
    fn maybe_auto_compact(&mut self) -> Result<()> {
        if let Some(threshold) = self.auto_compact_runs {
            if self.sealed_deltas.len() >= threshold {
                self.compact_delta()?;
            }
        }
        Ok(())
    }

    /// All queryable segments: immutable base segments, the sealed delta runs,
    /// then the active buffer. Gates and the tombstone/dedup pass apply uniformly
    /// across all of them.
    fn live_segments(&self) -> Vec<&Segment> {
        let mut segments: Vec<&Segment> =
            self.segments.iter().map(|loaded| &loaded.segment).collect();
        segments.extend(self.sealed_deltas.iter());
        if let Some(delta) = &self.delta_segment {
            segments.push(delta);
        }
        segments
    }

    /// Remove tombstoned ids and de-duplicate by capsule id (keeping the
    /// highest-scoring occurrence), then re-apply the result limit.
    fn apply_tombstones_and_dedup(&self, frame: &mut MemoryFrame, limit: usize) {
        let mut seen: HashSet<u128> = HashSet::new();
        frame
            .capsules
            .retain(|c| !self.tombstones.contains(&c.capsule_id) && seen.insert(c.capsule_id));
        frame
            .conflicts
            .retain(|c| !self.tombstones.contains(&c.capsule_id));
        frame.capsules.truncate(limit);
    }

    pub fn query(&self, request: &QueryRequest) -> Result<QueryResponse> {
        let started = Instant::now();
        let probe = request.to_probe()?;
        let mut frame = match request.mode {
            RetrievalMode::L5m => self.retrieve_l5m(&probe)?,
            RetrievalMode::ParentAggregate => self.retrieve_parent_aggregate(&probe)?,
            RetrievalMode::HybridBm25L5m | RetrievalMode::RrfFusionParent => {
                self.retrieve_fusion(&probe, request.mode == RetrievalMode::RrfFusionParent)
            }
        };
        self.apply_tombstones_and_dedup(&mut frame, probe.max_capsules);
        let elapsed = started.elapsed();
        self.metrics.record_query(
            frame.capsules.len(),
            frame.coverage.candidate_count_before_scoring,
            elapsed.as_nanos() as u64,
        );
        self.metrics
            .record_query_for(request.tenant_id, frame.capsules.len());
        Ok(QueryResponse {
            frame,
            mode: request.mode,
            config_hash: request.config_hash(),
            segment_count: self.live_segments().len(),
            segment_metadata: self.segment_metadata(),
            total_retrieval_ns: elapsed.as_nanos(),
        })
    }

    fn retrieve_l5m(&self, probe: &MemoryProbe) -> Result<MemoryFrame> {
        let mut frames = Vec::new();
        for segment in self.live_segments() {
            frames.push(crate::retrieve(segment, probe)?);
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

        for segment in self.live_segments() {
            epoch = epoch.max(segment.epoch());
            for capsule in segment.capsules() {
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
                let semantic = score_capsule(segment, capsule, probe);
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

    fn segment_metadata(&self) -> Vec<SegmentMetadata> {
        self.segments
            .iter()
            .map(|loaded| SegmentMetadata {
                path: loaded.path.clone().unwrap_or_default(),
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
    // Newest-tier-wins de-duplication by capsule id. Frames arrive in
    // `live_segments` order (base → sealed runs → active buffer = oldest →
    // newest), so a later copy of the same id overwrites an earlier one in place.
    // This makes an updated/reinserted capsule resolve to its freshest version
    // regardless of which tier the stale copy lives in. First-seen position is
    // preserved so the subsequent (stable) score sort is deterministic.
    let mut slot_of: HashMap<u128, usize> = HashMap::new();
    for frame in frames.drain(..) {
        merged.epoch = merged.epoch.max(frame.epoch);
        merged.conflicts.extend(frame.conflicts);
        merged.coverage.exact_entity_match |= frame.coverage.exact_entity_match;
        merged.coverage.anchor_match_count += frame.coverage.anchor_match_count;
        merged.coverage.temporal_valid_count += frame.coverage.temporal_valid_count;
        merged.coverage.trust_floor_met_count += frame.coverage.trust_floor_met_count;
        merged.coverage.context_valid_count += frame.coverage.context_valid_count;
        merged.coverage.candidate_count_before_scoring +=
            frame.coverage.candidate_count_before_scoring;
        for capsule in frame.capsules {
            match slot_of.get(&capsule.capsule_id) {
                Some(&i) => merged.capsules[i] = capsule,
                None => {
                    slot_of.insert(capsule.capsule_id, merged.capsules.len());
                    merged.capsules.push(capsule);
                }
            }
        }
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
