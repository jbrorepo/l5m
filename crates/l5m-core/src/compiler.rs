use std::{
    fs,
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    probe::{extract_terms, residual_for_text, semantic_bits_for_text},
    relation::{RelationEdge, RelationKind},
    MemoryCapsule, Result,
};

pub(crate) const MAGIC: &[u8; 8] = b"L5MSEG01";
pub(crate) const VERSION: u32 = 1;
pub(crate) const HEADER_LEN: usize = 128;
pub(crate) const HASH_OFFSET: usize = 72;
pub(crate) const HASH_LEN: usize = 32;
// Spare header bytes (104..128 in v1) carry the optional dense-embedding area.
// Old segments have zeros here -> embedding_dim 0 -> no embeddings (compatible).
pub(crate) const EMBED_DIM_OFFSET: usize = 104;
pub(crate) const VECTOR_AREA_OFFSET: usize = 108;
pub(crate) const METADATA_LEN: usize = 340;
pub(crate) const RELATION_LEN: usize = 37;
pub(crate) const NONE_I64: i64 = i64::MAX;

pub struct CompileOptions {
    pub input_json: PathBuf,
    pub output_segment: PathBuf,
    pub epoch: u64,
}

pub fn compile_segment(options: CompileOptions) -> Result<()> {
    let built = build_segment_bytes(&options.input_json, options.epoch)?;
    if let Some(parent) = options.output_segment.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(&options.output_segment)?;
    file.write_all(&built.bytes)?;
    file.sync_all()?;
    write_manifest(&options.output_segment, &built)?;
    Ok(())
}

/// Compile and **encrypt at rest** in one step: the plaintext segment never
/// touches disk. Requires the `encryption` feature.
#[cfg(feature = "encryption")]
pub fn compile_segment_sealed(
    options: CompileOptions,
    key: &dyn crate::crypto::KeyProvider,
) -> Result<()> {
    let built = build_segment_bytes(&options.input_json, options.epoch)?;
    let sealed = crate::crypto::seal(&built.bytes, &key.key()?)?;
    if let Some(parent) = options.output_segment.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(&options.output_segment)?;
    file.write_all(&sealed)?;
    file.sync_all()?;
    write_manifest(&options.output_segment, &built)?;
    Ok(())
}

pub(crate) struct BuiltSegment {
    pub bytes: Vec<u8>,
    pub capsule_count: u64,
    pub tenant_id: u64,
    pub epoch: u64,
    pub hash: blake3::Hash,
}

fn write_manifest(output_segment: &std::path::Path, built: &BuiltSegment) -> Result<()> {
    let manifest = Manifest {
        epoch: built.epoch,
        capsule_count: built.capsule_count,
        tenant_id: built.tenant_id,
        segment_hash: hex32(built.hash.as_bytes()),
        build_time_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0),
    };
    fs::write(
        output_segment.with_extension("segment.manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;
    Ok(())
}

/// Write a segment file directly from already-built capsules (no JSON round
/// trip). Used by the durable store to checkpoint its live state.
pub fn compile_capsules(
    output_segment: &std::path::Path,
    capsules: Vec<MemoryCapsule>,
    epoch: u64,
) -> Result<()> {
    let built = assemble_segment(capsules, epoch)?;
    if let Some(parent) = output_segment.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(output_segment)?;
    file.write_all(&built.bytes)?;
    file.sync_all()?;
    write_manifest(output_segment, &built)?;
    Ok(())
}

pub(crate) fn build_segment_bytes(
    input_json: &std::path::Path,
    epoch: u64,
) -> Result<BuiltSegment> {
    let input = fs::read_to_string(input_json)?;
    let raw: Vec<RawCapsule> = serde_json::from_str(&input)?;
    let mut capsules = Vec::with_capacity(raw.len());
    for capsule in raw {
        capsules.push(capsule.into_capsule()?);
    }
    assemble_segment(capsules, epoch)
}

fn assemble_segment(mut capsules: Vec<MemoryCapsule>, epoch: u64) -> Result<BuiltSegment> {
    capsules.sort_by_key(|capsule| capsule.capsule_id);
    let tenant_id = capsules.first().map_or(0, |capsule| capsule.tenant_id);

    let mut string_area = Vec::new();
    let mut relation_area = Vec::new();
    let mut metadata_records = Vec::with_capacity(capsules.len());

    for capsule in &capsules {
        let claim = write_bytes(&mut string_area, capsule.claim.as_bytes());
        let evidence = write_bytes(&mut string_area, capsule.evidence.as_bytes());
        let source_uri = match &capsule.source_uri {
            Some(uri) => write_bytes(&mut string_area, uri.as_bytes()),
            None => BlobRef::none(),
        };
        let anchors = write_string_list(&mut string_area, &capsule.anchors);
        let entities = write_string_list(&mut string_area, &capsule.entities);
        let relation_offset = relation_area.len() as u64;
        for edge in &capsule.relation_edges {
            write_u128(&mut relation_area, edge.from);
            write_u128(&mut relation_area, edge.to);
            relation_area.push(edge.kind as u8);
            write_i16(&mut relation_area, edge.weight);
            relation_area.extend_from_slice(&[0, 0]);
        }
        metadata_records.push(MetadataRefs {
            claim,
            evidence,
            source_uri,
            anchors,
            entities,
            relation_offset,
            relation_count: capsule.relation_edges.len() as u32,
        });
    }

    // Optional dense embeddings: all present capsules must share one dimension.
    let embedding_dim = capsules
        .iter()
        .map(|c| c.embedding.len())
        .max()
        .unwrap_or(0);
    if embedding_dim > 0 {
        for capsule in &capsules {
            if capsule.embedding.len() != embedding_dim {
                return Err(crate::L5mError::Format(format!(
                    "inconsistent embedding dimension: capsule {} has {}, expected {embedding_dim}",
                    capsule.capsule_id,
                    capsule.embedding.len()
                )));
            }
        }
    }

    let metadata_offset = HEADER_LEN as u64;
    let string_offset = metadata_offset + (capsules.len() * METADATA_LEN) as u64;
    let relation_offset = string_offset + string_area.len() as u64;
    let index_offset = relation_offset + relation_area.len() as u64;
    // Index summary is exactly 16 bytes (marker + count); the vector area follows.
    let vector_offset = index_offset + 16;

    let mut bytes = vec![0u8; HEADER_LEN];
    write_header_prefix(
        &mut bytes,
        HeaderFields {
            epoch,
            tenant_id,
            capsule_count: capsules.len() as u64,
            metadata_offset,
            string_offset,
            relation_offset,
            index_offset,
            embedding_dim: embedding_dim as u32,
            vector_offset,
        },
    );
    for (capsule, refs) in capsules.iter().zip(&metadata_records) {
        write_metadata(&mut bytes, capsule, refs);
    }
    bytes.extend_from_slice(&string_area);
    bytes.extend_from_slice(&relation_area);
    write_index_summary(&mut bytes, &capsules);
    debug_assert_eq!(bytes.len() as u64, vector_offset);
    if embedding_dim > 0 {
        for capsule in &capsules {
            for value in &capsule.embedding {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    }

    let hash = segment_hash(&bytes);
    bytes[HASH_OFFSET..HASH_OFFSET + HASH_LEN].copy_from_slice(hash.as_bytes());

    Ok(BuiltSegment {
        capsule_count: capsules.len() as u64,
        tenant_id,
        epoch,
        hash,
        bytes,
    })
}

/// Build a single `MemoryCapsule` (computing fingerprints, anchors, entities,
/// and hashes) from the same JSON shape `compile_segment` accepts. Used by the
/// mutable store to ingest new memories at runtime with identical semantics.
pub fn capsule_from_json(value: &serde_json::Value) -> Result<MemoryCapsule> {
    let raw: RawCapsule = serde_json::from_value(value.clone())?;
    raw.into_capsule()
}

/// Serialize a built capsule back into the JSON shape `capsule_from_json`
/// accepts. Used by the durable write-ahead log so a replay reconstructs an
/// equivalent capsule. (Fingerprints are recomputed deterministically on
/// replay, so they need not be stored.)
pub fn capsule_to_json(capsule: &MemoryCapsule) -> serde_json::Value {
    let mut value = serde_json::json!({
        "capsule_id": capsule.capsule_id.to_string(),
        "tenant_id": capsule.tenant_id,
        "claim": capsule.claim,
        "evidence": capsule.evidence,
        "source_id": capsule.source_id,
        "valid_from": capsule.valid_from,
        "observed_at": capsule.observed_at,
        "last_verified_at": capsule.last_verified_at,
        "context_mask": format!("{:#x}", capsule.context_mask),
        "policy_mask": format!("{:#x}", capsule.policy_mask),
        "trust_level": capsule.trust_level,
        "classification": capsule.classification,
        "poison_risk": capsule.poison_risk,
        "anchors": capsule.anchors,
        "entities": capsule.entities,
    });
    if let Some(uri) = &capsule.source_uri {
        value["source_uri"] = serde_json::json!(uri);
    }
    if let Some(until) = capsule.valid_until {
        value["valid_until"] = serde_json::json!(until);
    }
    if !capsule.embedding.is_empty() {
        value["embedding"] = serde_json::json!(capsule.embedding);
    }
    if !capsule.relation_edges.is_empty() {
        value["relation_edges"] = serde_json::json!(capsule
            .relation_edges
            .iter()
            .map(|e| serde_json::json!({
                "from": e.from.to_string(),
                "to": e.to.to_string(),
                "kind": e.kind,
                "weight": e.weight,
            }))
            .collect::<Vec<_>>());
    }
    value
}

#[derive(Debug, Deserialize)]
struct RawCapsule {
    capsule_id: String,
    tenant_id: u64,
    claim: String,
    evidence: String,
    source_id: u64,
    #[serde(default)]
    source_uri: Option<String>,
    #[serde(default)]
    anchors: Option<Vec<String>>,
    #[serde(default)]
    entities: Option<Vec<String>>,
    #[serde(default)]
    embedding: Option<Vec<f32>>,
    valid_from: i64,
    #[serde(default)]
    valid_until: Option<i64>,
    observed_at: i64,
    last_verified_at: i64,
    context_mask: String,
    policy_mask: String,
    trust_level: u8,
    classification: u8,
    poison_risk: u8,
    #[serde(default)]
    relation_edges: Vec<RawRelationEdge>,
}

impl RawCapsule {
    fn into_capsule(self) -> Result<MemoryCapsule> {
        let mut anchors = self
            .anchors
            .unwrap_or_else(|| extract_terms(&format!("{} {}", self.claim, self.evidence)));
        normalize_terms(&mut anchors);
        let mut entities = self.entities.unwrap_or_else(|| anchors.clone());
        normalize_terms(&mut entities);
        let semantic_text = format!("{} {} {}", self.claim, self.evidence, anchors.join(" "));
        let source_material = self
            .source_uri
            .as_deref()
            .unwrap_or(self.evidence.as_str())
            .as_bytes()
            .to_vec();
        let content_material = format!("{}\n{}", self.claim, self.evidence);
        Ok(MemoryCapsule {
            capsule_id: parse_u128(&self.capsule_id)?,
            tenant_id: self.tenant_id,
            claim: self.claim,
            evidence: self.evidence,
            source_id: self.source_id,
            source_uri: self.source_uri,
            source_hash: *blake3::hash(&source_material).as_bytes(),
            semantic_bits: semantic_bits_for_text(&semantic_text),
            residual: residual_for_text(&semantic_text),
            embedding: self.embedding.unwrap_or_default(),
            anchors,
            entities,
            valid_from: self.valid_from,
            valid_until: self.valid_until,
            observed_at: self.observed_at,
            last_verified_at: self.last_verified_at,
            context_mask: parse_u128(&self.context_mask)?,
            policy_mask: parse_u128(&self.policy_mask)?,
            trust_level: self.trust_level,
            classification: self.classification,
            poison_risk: self.poison_risk,
            relation_edges: self
                .relation_edges
                .into_iter()
                .map(RawRelationEdge::into_edge)
                .collect::<Result<Vec<_>>>()?,
            content_hash: *blake3::hash(content_material.as_bytes()).as_bytes(),
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawRelationEdge {
    from: String,
    to: String,
    kind: RelationKind,
    weight: i16,
}

impl RawRelationEdge {
    fn into_edge(self) -> Result<RelationEdge> {
        Ok(RelationEdge {
            from: parse_u128(&self.from)?,
            to: parse_u128(&self.to)?,
            kind: self.kind,
            weight: self.weight,
        })
    }
}

#[derive(Debug, Serialize)]
struct Manifest {
    epoch: u64,
    capsule_count: u64,
    tenant_id: u64,
    segment_hash: String,
    build_time_unix: u64,
}

#[derive(Clone, Copy)]
struct BlobRef {
    offset: u64,
    len: u32,
}

impl BlobRef {
    fn none() -> Self {
        Self {
            offset: u64::MAX,
            len: u32::MAX,
        }
    }
}

struct MetadataRefs {
    claim: BlobRef,
    evidence: BlobRef,
    source_uri: BlobRef,
    anchors: BlobRef,
    entities: BlobRef,
    relation_offset: u64,
    relation_count: u32,
}

struct HeaderFields {
    epoch: u64,
    tenant_id: u64,
    capsule_count: u64,
    metadata_offset: u64,
    string_offset: u64,
    relation_offset: u64,
    index_offset: u64,
    embedding_dim: u32,
    vector_offset: u64,
}

fn write_header_prefix(bytes: &mut [u8], fields: HeaderFields) {
    bytes[0..8].copy_from_slice(MAGIC);
    bytes[8..12].copy_from_slice(&VERSION.to_le_bytes());
    bytes[12..16].copy_from_slice(&(HEADER_LEN as u32).to_le_bytes());
    bytes[16..24].copy_from_slice(&fields.epoch.to_le_bytes());
    bytes[24..32].copy_from_slice(&fields.tenant_id.to_le_bytes());
    bytes[32..40].copy_from_slice(&fields.capsule_count.to_le_bytes());
    bytes[40..48].copy_from_slice(&fields.metadata_offset.to_le_bytes());
    bytes[48..56].copy_from_slice(&fields.string_offset.to_le_bytes());
    bytes[56..64].copy_from_slice(&fields.relation_offset.to_le_bytes());
    bytes[64..72].copy_from_slice(&fields.index_offset.to_le_bytes());
    bytes[EMBED_DIM_OFFSET..EMBED_DIM_OFFSET + 4]
        .copy_from_slice(&fields.embedding_dim.to_le_bytes());
    bytes[VECTOR_AREA_OFFSET..VECTOR_AREA_OFFSET + 8]
        .copy_from_slice(&fields.vector_offset.to_le_bytes());
}

fn write_metadata(bytes: &mut Vec<u8>, capsule: &MemoryCapsule, refs: &MetadataRefs) {
    write_u128(bytes, capsule.capsule_id);
    write_u64(bytes, capsule.tenant_id);
    write_u64(bytes, capsule.source_id);
    bytes.extend_from_slice(&capsule.source_hash);
    for value in capsule.semantic_bits {
        write_u64(bytes, value);
    }
    for value in capsule.residual {
        bytes.push(value as u8);
    }
    write_i64(bytes, capsule.valid_from);
    write_i64(bytes, capsule.valid_until.unwrap_or(NONE_I64));
    write_i64(bytes, capsule.observed_at);
    write_i64(bytes, capsule.last_verified_at);
    write_u128(bytes, capsule.context_mask);
    write_u128(bytes, capsule.policy_mask);
    bytes.extend_from_slice(&[
        capsule.trust_level,
        capsule.classification,
        capsule.poison_risk,
        0,
    ]);
    write_blob_ref(bytes, refs.claim);
    write_blob_ref(bytes, refs.evidence);
    write_blob_ref(bytes, refs.source_uri);
    write_blob_ref(bytes, refs.anchors);
    write_blob_ref(bytes, refs.entities);
    write_u64(bytes, refs.relation_offset);
    write_u32(bytes, refs.relation_count);
    bytes.extend_from_slice(&capsule.content_hash);
    bytes.extend_from_slice(&[0; 8]);
    debug_assert_eq!(bytes.len() % METADATA_LEN, HEADER_LEN % METADATA_LEN);
}

fn write_blob_ref(bytes: &mut Vec<u8>, blob: BlobRef) {
    write_u64(bytes, blob.offset);
    write_u32(bytes, blob.len);
}

fn write_bytes(area: &mut Vec<u8>, bytes: &[u8]) -> BlobRef {
    let offset = area.len() as u64;
    area.extend_from_slice(bytes);
    BlobRef {
        offset,
        len: bytes.len() as u32,
    }
}

fn write_string_list(area: &mut Vec<u8>, values: &[String]) -> BlobRef {
    let offset = area.len() as u64;
    write_u32(area, values.len() as u32);
    for value in values {
        write_u32(area, value.len() as u32);
        area.extend_from_slice(value.as_bytes());
    }
    BlobRef {
        offset,
        len: (area.len() as u64 - offset) as u32,
    }
}

fn write_index_summary(bytes: &mut Vec<u8>, capsules: &[MemoryCapsule]) {
    bytes.extend_from_slice(b"L5MIDX01");
    write_u64(bytes, capsules.len() as u64);
}

pub(crate) fn segment_hash(bytes: &[u8]) -> blake3::Hash {
    let mut hasher = blake3::Hasher::new();
    if bytes.len() < HASH_OFFSET + HASH_LEN {
        hasher.update(bytes);
    } else {
        // The segment authenticates itself, so the stored hash field is treated
        // as zero bytes while computing the BLAKE3 digest.
        hasher.update(&bytes[..HASH_OFFSET]);
        hasher.update(&[0; HASH_LEN]);
        hasher.update(&bytes[HASH_OFFSET + HASH_LEN..]);
    }
    hasher.finalize()
}

fn normalize_terms(values: &mut Vec<String>) {
    *values = values
        .iter()
        .flat_map(|value| extract_terms(value))
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
}

pub fn parse_u128(value: &str) -> Result<u128> {
    let trimmed = value.trim();
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u128::from_str_radix(hex, 16)
    } else {
        trimmed.parse()
    }
    .map_err(|err| crate::L5mError::Format(format!("invalid u128 '{value}': {err}")))
}

fn hex32(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn write_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn write_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn write_i64(bytes: &mut Vec<u8>, value: i64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn write_i16(bytes: &mut Vec<u8>, value: i16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn write_u128(bytes: &mut Vec<u8>, value: u128) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
