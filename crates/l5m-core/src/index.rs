use std::collections::HashMap;

use crate::{MemoryCapsule, RelationKind};

/// Compact, cache-dense copy of just the fields the gate scan reads. Iterating
/// these (~56 bytes) instead of the full `MemoryCapsule` (~350 bytes) makes the
/// per-query O(N) gate scan memory-bandwidth efficient.
#[derive(Clone, Copy, Debug)]
pub struct GateRow {
    pub tenant_id: u64,
    pub context_mask: u128,
    pub policy_mask: u128,
    pub valid_from: i64,
    pub valid_until: Option<i64>,
    pub trust_level: u8,
}

/// Number of 16-bit bands the 256-bit fingerprint is split into for LSH.
pub const LSH_BANDS: usize = 16;

/// Extract band `band` (a 16-bit slice) from a 256-bit fingerprint.
#[inline]
pub fn band_key(bits: &[u64; 4], band: usize) -> u16 {
    let word = bits[band / 4];
    let slice = band % 4;
    ((word >> (16 * slice)) & 0xffff) as u16
}

/// Locality-sensitive hash index over fingerprints: one table per 16-bit band,
/// mapping a band value to the ordinals carrying it. Two fingerprints that are
/// close in Hamming distance share many band values, so a probe finds its near
/// neighbors by visiting only the postings of its own band values — sublinear
/// candidate generation instead of an O(N) scan.
#[derive(Clone, Debug, Default)]
pub struct SemanticLsh {
    tables: Vec<HashMap<u16, Vec<u32>>>,
}

impl SemanticLsh {
    pub fn build(fingerprints: &[[u64; 4]]) -> Self {
        let mut tables = vec![HashMap::<u16, Vec<u32>>::new(); LSH_BANDS];
        for (ordinal, bits) in fingerprints.iter().enumerate() {
            for (band, table) in tables.iter_mut().enumerate() {
                table
                    .entry(band_key(bits, band))
                    .or_default()
                    .push(ordinal as u32);
            }
        }
        Self { tables }
    }

    /// Visit candidate ordinals whose fingerprint shares at least one band value
    /// with `bits`. Ordinals may be visited multiple times (once per shared
    /// band); the caller dedups (e.g. via a seen bitset).
    pub fn for_each_candidate(&self, bits: &[u64; 4], mut visit: impl FnMut(usize)) {
        for (band, table) in self.tables.iter().enumerate() {
            if let Some(postings) = table.get(&band_key(bits, band)) {
                for &ordinal in postings {
                    visit(ordinal as usize);
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tables.iter().all(HashMap::is_empty)
    }
}

/// Cosine-LSH over dense embeddings (random-hyperplane SimHash). Each embedding
/// gets a 64-bit signature (sign of its projection onto 64 fixed random
/// hyperplanes); banding the signature into 16-bit chunks lets a query embedding
/// find its near neighbors by visiting only matching bands — sublinear dense
/// candidate generation within a large tenant, instead of scoring everything.
#[derive(Clone, Debug, Default)]
pub struct EmbeddingLsh {
    dim: usize,
    hyperplanes: Vec<Vec<f32>>,
    tables: Vec<HashMap<u16, Vec<u32>>>,
}

const SIG_BANDS: usize = 4; // 4 × 16-bit bands = a 64-bit signature

impl EmbeddingLsh {
    pub fn build(embeddings: &[&[f32]]) -> Option<Self> {
        let dim = embeddings.iter().find(|e| !e.is_empty())?.len();
        if dim == 0 {
            return None;
        }
        // Deterministic ±1 hyperplanes (64 of them) so build and query agree.
        let mut state = 0x1234_5678_9abc_def0u64;
        let hyperplanes: Vec<Vec<f32>> = (0..(SIG_BANDS * 16))
            .map(|_| {
                (0..dim)
                    .map(|_| {
                        if splitmix64(&mut state) & 1 == 0 {
                            1.0
                        } else {
                            -1.0
                        }
                    })
                    .collect()
            })
            .collect();
        let mut tables = vec![HashMap::<u16, Vec<u32>>::new(); SIG_BANDS];
        for (ordinal, emb) in embeddings.iter().enumerate() {
            if emb.len() != dim {
                continue;
            }
            let sig = signature(emb, &hyperplanes);
            for (band, table) in tables.iter_mut().enumerate() {
                let key = ((sig >> (16 * band)) & 0xffff) as u16;
                table.entry(key).or_default().push(ordinal as u32);
            }
        }
        Some(Self {
            dim,
            hyperplanes,
            tables,
        })
    }

    /// Visit candidate ordinals whose embedding shares a SimHash band with
    /// `query` (i.e. are likely cosine-near). Caller dedups + gate-filters.
    pub fn for_each_candidate(&self, query: &[f32], mut visit: impl FnMut(usize)) {
        if query.len() != self.dim {
            return;
        }
        let sig = signature(query, &self.hyperplanes);
        for (band, table) in self.tables.iter().enumerate() {
            let key = ((sig >> (16 * band)) & 0xffff) as u16;
            if let Some(postings) = table.get(&key) {
                for &ordinal in postings {
                    visit(ordinal as usize);
                }
            }
        }
    }
}

fn signature(emb: &[f32], hyperplanes: &[Vec<f32>]) -> u64 {
    let mut sig = 0u64;
    for (i, plane) in hyperplanes.iter().enumerate() {
        let dot: f32 = emb.iter().zip(plane).map(|(a, b)| a * b).sum();
        if dot >= 0.0 {
            sig |= 1u64 << i;
        }
    }
    sig
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[derive(Clone, Debug, Default)]
pub struct SegmentIndex {
    pub anchors: HashMap<u64, Vec<usize>>,
    pub entities: HashMap<u64, Vec<usize>>,
    pub semantic_buckets: HashMap<u16, Vec<usize>>,
    pub supersedes_by_target: HashMap<u128, Vec<usize>>,
    pub by_id: HashMap<u128, usize>,
    /// Columnar gate fields, indexed by ordinal (SoA hot-path layout).
    pub gate_rows: Vec<GateRow>,
    /// Columnar 256-bit semantic fingerprints, indexed by ordinal.
    pub fingerprints: Vec<[u64; 4]>,
    /// `true` at ordinals that are the target of some supersede edge — the only
    /// capsules for which the (probe-dependent) supersession check must run.
    pub supersede_target: Vec<bool>,
    /// LSH index over `fingerprints` for sublinear semantic candidate gen.
    pub lsh: SemanticLsh,
    /// Ordinals grouped by tenant. The tenant gate is the security boundary AND
    /// the first selectivity win: a probe scans only its own tenant's ordinals
    /// instead of the whole segment.
    pub tenant_postings: HashMap<u64, Vec<u32>>,
    /// Cosine-LSH over dense embeddings (present only when capsules carry them).
    pub embedding_lsh: Option<EmbeddingLsh>,
}

impl SegmentIndex {
    pub fn build(capsules: &[MemoryCapsule]) -> Self {
        let mut index = Self::default();
        index.gate_rows.reserve(capsules.len());
        index.fingerprints.reserve(capsules.len());
        index.supersede_target = vec![false; capsules.len()];
        for (ordinal, capsule) in capsules.iter().enumerate() {
            index.by_id.insert(capsule.capsule_id, ordinal);
            index.gate_rows.push(GateRow {
                tenant_id: capsule.tenant_id,
                context_mask: capsule.context_mask,
                policy_mask: capsule.policy_mask,
                valid_from: capsule.valid_from,
                valid_until: capsule.valid_until,
                trust_level: capsule.trust_level,
            });
            index.fingerprints.push(capsule.semantic_bits);
            index
                .tenant_postings
                .entry(capsule.tenant_id)
                .or_default()
                .push(ordinal as u32);
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
        // Mark the (rare) capsules that are supersede targets so the gate scan
        // only pays for the probe-dependent supersession check where it matters.
        for target_id in index.supersedes_by_target.keys() {
            if let Some(&ordinal) = index.by_id.get(target_id) {
                if let Some(slot) = index.supersede_target.get_mut(ordinal) {
                    *slot = true;
                }
            }
        }
        index.lsh = SemanticLsh::build(&index.fingerprints);
        if capsules.iter().any(|c| !c.embedding.is_empty()) {
            let embeddings: Vec<&[f32]> = capsules.iter().map(|c| c.embedding.as_slice()).collect();
            index.embedding_lsh = EmbeddingLsh::build(&embeddings);
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
