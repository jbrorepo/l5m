use std::{fs::File, path::Path};

use memmap2::Mmap;

use crate::{
    compiler::{
        segment_hash, HASH_LEN, HASH_OFFSET, HEADER_LEN, MAGIC, METADATA_LEN, NONE_I64,
        RELATION_LEN, VERSION,
    },
    index::SegmentIndex,
    relation::{RelationEdge, RelationKind},
    L5mError, MemoryCapsule, Result,
};

pub struct Segment {
    _mmap: Mmap,
    epoch: u64,
    tenant_id: u64,
    capsules: Vec<MemoryCapsule>,
    index: SegmentIndex,
}

impl Segment {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = File::open(path)?;
        // SAFETY: The file is mapped read-only and the Mmap is kept alive inside Segment.
        // All access below is bounds-checked before slicing.
        let mmap = unsafe { Mmap::map(&file)? };
        validate_header(&mmap)?;
        let stored_hash = &mmap[HASH_OFFSET..HASH_OFFSET + HASH_LEN];
        let computed_hash = segment_hash(&mmap);
        if stored_hash != computed_hash.as_bytes() {
            return Err(L5mError::Format("segment hash mismatch".to_string()));
        }

        let epoch = read_u64(&mmap, 16)?;
        let tenant_id = read_u64(&mmap, 24)?;
        let capsule_count = read_u64(&mmap, 32)? as usize;
        let metadata_offset = read_u64(&mmap, 40)? as usize;
        let string_offset = read_u64(&mmap, 48)? as usize;
        let relation_offset = read_u64(&mmap, 56)? as usize;
        let index_offset = read_u64(&mmap, 64)? as usize;

        validate_sections(
            &mmap,
            capsule_count,
            metadata_offset,
            string_offset,
            relation_offset,
            index_offset,
        )?;

        let mut capsules = Vec::with_capacity(capsule_count);
        for ordinal in 0..capsule_count {
            let base = metadata_offset + ordinal * METADATA_LEN;
            capsules.push(read_capsule(
                &mmap,
                base,
                string_offset,
                relation_offset,
                index_offset,
            )?);
        }
        let index = SegmentIndex::build(&capsules);
        if index.by_id.len() != capsules.len() {
            return Err(L5mError::Format(
                "duplicate capsule IDs in segment".to_string(),
            ));
        }
        Ok(Self {
            _mmap: mmap,
            epoch,
            tenant_id,
            capsules,
            index,
        })
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn tenant_id(&self) -> u64 {
        self.tenant_id
    }

    pub fn capsule_count(&self) -> usize {
        self.capsules.len()
    }

    pub fn capsule_by_id(&self, _capsule_id: u128) -> Option<&MemoryCapsule> {
        self.index
            .by_id
            .get(&_capsule_id)
            .and_then(|ordinal| self.capsules.get(*ordinal))
    }

    pub fn capsule(&self, ordinal: usize) -> Option<&MemoryCapsule> {
        self.capsules.get(ordinal)
    }

    pub fn capsules(&self) -> &[MemoryCapsule] {
        &self.capsules
    }

    pub fn index(&self) -> &SegmentIndex {
        &self.index
    }

    pub fn relations_from(&self, capsule_id: u128) -> &[RelationEdge] {
        self.capsule_by_id(capsule_id)
            .map(|capsule| capsule.relation_edges.as_slice())
            .unwrap_or(&[])
    }
}

fn validate_header(bytes: &[u8]) -> Result<()> {
    if bytes.len() < HEADER_LEN {
        return Err(L5mError::Format(
            "segment is shorter than header".to_string(),
        ));
    }
    if &bytes[0..8] != MAGIC {
        return Err(L5mError::Format("invalid segment magic".to_string()));
    }
    let version = read_u32(bytes, 8)?;
    if version != VERSION {
        return Err(L5mError::Format(format!(
            "unsupported segment version {version}"
        )));
    }
    let header_len = read_u32(bytes, 12)? as usize;
    if header_len != HEADER_LEN {
        return Err(L5mError::Format(format!(
            "unsupported header length {header_len}"
        )));
    }
    Ok(())
}

fn validate_sections(
    bytes: &[u8],
    capsule_count: usize,
    metadata_offset: usize,
    string_offset: usize,
    relation_offset: usize,
    index_offset: usize,
) -> Result<()> {
    let metadata_len = capsule_count
        .checked_mul(METADATA_LEN)
        .ok_or_else(|| L5mError::Format("metadata table length overflows usize".to_string()))?;
    // Version 1 keeps fixed-width metadata directly after the header. Enforcing
    // monotonic, contiguous sections prevents blob references from bleeding
    // into relation or index bytes after a malicious header edit.
    let expected_string_offset = metadata_offset
        .checked_add(metadata_len)
        .ok_or_else(|| L5mError::Format("metadata table end overflows usize".to_string()))?;
    if metadata_offset != HEADER_LEN
        || expected_string_offset != string_offset
        || string_offset > relation_offset
        || relation_offset > index_offset
        || index_offset > bytes.len()
    {
        return Err(L5mError::Format(
            "invalid section offsets in segment header".to_string(),
        ));
    }
    checked_range(bytes, metadata_offset, metadata_len)?;
    checked_range(bytes, string_offset, relation_offset - string_offset)?;
    checked_range(bytes, relation_offset, index_offset - relation_offset)?;
    checked_range(bytes, index_offset, 16)?;
    if &bytes[index_offset..index_offset + 8] != b"L5MIDX01" {
        return Err(L5mError::Format("invalid index summary marker".to_string()));
    }
    let indexed_count = read_u64(bytes, index_offset + 8)? as usize;
    if indexed_count != capsule_count {
        return Err(L5mError::Format(format!(
            "index summary capsule count {indexed_count} does not match header count {capsule_count}"
        )));
    }
    Ok(())
}

fn read_capsule(
    bytes: &[u8],
    base: usize,
    string_base: usize,
    relation_base: usize,
    relation_end: usize,
) -> Result<MemoryCapsule> {
    let capsule_id = read_u128(bytes, base)?;
    let tenant_id = read_u64(bytes, base + 16)?;
    let source_id = read_u64(bytes, base + 24)?;
    let source_hash = read_array32(bytes, base + 32)?;
    let semantic_bits = [
        read_u64(bytes, base + 64)?,
        read_u64(bytes, base + 72)?,
        read_u64(bytes, base + 80)?,
        read_u64(bytes, base + 88)?,
    ];
    let mut residual = [0i8; 64];
    for (index, value) in residual.iter_mut().enumerate() {
        *value = bytes[base + 96 + index] as i8;
    }
    let valid_from = read_i64(bytes, base + 160)?;
    let valid_until_raw = read_i64(bytes, base + 168)?;
    let observed_at = read_i64(bytes, base + 176)?;
    let last_verified_at = read_i64(bytes, base + 184)?;
    let context_mask = read_u128(bytes, base + 192)?;
    let policy_mask = read_u128(bytes, base + 208)?;
    let trust_level = bytes[base + 224];
    let classification = bytes[base + 225];
    let poison_risk = bytes[base + 226];
    let claim = read_string(bytes, string_base, relation_base, base + 228)?;
    let evidence = read_string(bytes, string_base, relation_base, base + 240)?;
    let source_uri = read_optional_string(bytes, string_base, relation_base, base + 252)?;
    let anchors = read_string_list(bytes, string_base, relation_base, base + 264)?;
    let entities = read_string_list(bytes, string_base, relation_base, base + 276)?;
    let relation_offset = read_u64(bytes, base + 288)? as usize;
    let relation_count = read_u32(bytes, base + 296)? as usize;
    let content_hash = read_array32(bytes, base + 300)?;
    validate_capsule_hashes(
        capsule_id,
        &claim,
        &evidence,
        source_uri.as_deref(),
        source_hash,
        content_hash,
    )?;
    let mut relation_edges = Vec::with_capacity(relation_count);
    let relation_bytes = relation_count
        .checked_mul(RELATION_LEN)
        .ok_or_else(|| L5mError::Format("relation list length overflows usize".to_string()))?;
    let relation_start = relation_base
        .checked_add(relation_offset)
        .ok_or_else(|| L5mError::Format("relation offset overflows usize".to_string()))?;
    checked_range_with_limit(relation_start, relation_bytes, relation_end)?;
    for index in 0..relation_count {
        let edge = read_relation(
            bytes,
            relation_base + relation_offset + index * RELATION_LEN,
        )?;
        if edge.from != capsule_id {
            return Err(L5mError::Format(format!(
                "relation source {} does not match capsule {capsule_id}",
                edge.from
            )));
        }
        relation_edges.push(edge);
    }
    Ok(MemoryCapsule {
        capsule_id,
        tenant_id,
        claim,
        evidence,
        source_id,
        source_uri,
        source_hash,
        semantic_bits,
        residual,
        anchors,
        entities,
        valid_from,
        valid_until: (valid_until_raw != NONE_I64).then_some(valid_until_raw),
        observed_at,
        last_verified_at,
        context_mask,
        policy_mask,
        trust_level,
        classification,
        poison_risk,
        relation_edges,
        content_hash,
    })
}

fn validate_capsule_hashes(
    capsule_id: u128,
    claim: &str,
    evidence: &str,
    source_uri: Option<&str>,
    source_hash: [u8; 32],
    content_hash: [u8; 32],
) -> Result<()> {
    let mut content_hasher = blake3::Hasher::new();
    content_hasher.update(claim.as_bytes());
    content_hasher.update(b"\n");
    content_hasher.update(evidence.as_bytes());
    if content_hasher.finalize().as_bytes() != &content_hash {
        return Err(L5mError::Format(format!(
            "content hash mismatch for capsule {capsule_id}"
        )));
    }

    let source_material = source_uri
        .map(str::as_bytes)
        .unwrap_or_else(|| evidence.as_bytes());
    if blake3::hash(source_material).as_bytes() != &source_hash {
        return Err(L5mError::Format(format!(
            "source hash mismatch for capsule {capsule_id}"
        )));
    }
    Ok(())
}

fn read_relation(bytes: &[u8], offset: usize) -> Result<RelationEdge> {
    checked_range(bytes, offset, RELATION_LEN)?;
    let kind = match bytes[offset + 32] {
        0 => RelationKind::Supports,
        1 => RelationKind::Contradicts,
        2 => RelationKind::Supersedes,
        3 => RelationKind::DependsOn,
        4 => RelationKind::DerivedFrom,
        5 => RelationKind::DuplicateOf,
        value => {
            return Err(L5mError::Format(format!(
                "invalid relation kind byte {value}"
            )))
        }
    };
    Ok(RelationEdge {
        from: read_u128(bytes, offset)?,
        to: read_u128(bytes, offset + 16)?,
        kind,
        weight: read_i16(bytes, offset + 33)?,
    })
}

fn read_optional_string(
    bytes: &[u8],
    string_base: usize,
    string_end: usize,
    offset: usize,
) -> Result<Option<String>> {
    let blob_offset = read_u64(bytes, offset)?;
    let blob_len = read_u32(bytes, offset + 8)?;
    if blob_offset == u64::MAX && blob_len == u32::MAX {
        Ok(None)
    } else {
        read_string_at(
            bytes,
            string_base,
            string_end,
            blob_offset as usize,
            blob_len as usize,
        )
        .map(Some)
    }
}

fn read_string(
    bytes: &[u8],
    string_base: usize,
    string_end: usize,
    offset: usize,
) -> Result<String> {
    let blob_offset = read_u64(bytes, offset)? as usize;
    let blob_len = read_u32(bytes, offset + 8)? as usize;
    read_string_at(bytes, string_base, string_end, blob_offset, blob_len)
}

fn read_string_at(
    bytes: &[u8],
    string_base: usize,
    string_end: usize,
    offset: usize,
    len: usize,
) -> Result<String> {
    let start = string_base
        .checked_add(offset)
        .ok_or_else(|| L5mError::Format("string offset overflows usize".to_string()))?;
    checked_range(bytes, start, len)?;
    checked_range_with_limit(start, len, string_end)?;
    std::str::from_utf8(&bytes[start..start + len])
        .map(str::to_string)
        .map_err(|err| L5mError::Format(format!("invalid utf-8 string payload: {err}")))
}

fn read_string_list(
    bytes: &[u8],
    string_base: usize,
    string_end: usize,
    offset: usize,
) -> Result<Vec<String>> {
    let blob_offset = read_u64(bytes, offset)? as usize;
    let blob_len = read_u32(bytes, offset + 8)? as usize;
    let start = string_base
        .checked_add(blob_offset)
        .ok_or_else(|| L5mError::Format("string list offset overflows usize".to_string()))?;
    checked_range(bytes, start, blob_len)?;
    checked_range_with_limit(start, blob_len, string_end)?;
    let end = start + blob_len;
    let mut cursor = start;
    // String lists are length-delimited inside the shared string area:
    // u32 count, then count repetitions of u32 byte length plus UTF-8 bytes.
    let count = read_u32(bytes, cursor)? as usize;
    cursor += 4;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        if cursor >= end {
            return Err(L5mError::Format("truncated string list".to_string()));
        }
        let len = read_u32(bytes, cursor)? as usize;
        cursor += 4;
        checked_range(bytes, cursor, len)?;
        values.push(
            std::str::from_utf8(&bytes[cursor..cursor + len])
                .map(str::to_string)
                .map_err(|err| L5mError::Format(format!("invalid utf-8 list string: {err}")))?,
        );
        cursor += len;
    }
    if cursor != end {
        return Err(L5mError::Format(
            "string list contains trailing bytes".to_string(),
        ));
    }
    Ok(values)
}

fn checked_range(bytes: &[u8], offset: usize, len: usize) -> Result<()> {
    offset
        .checked_add(len)
        .filter(|end| *end <= bytes.len())
        .map(|_| ())
        .ok_or_else(|| L5mError::Format("segment offset out of bounds".to_string()))
}

fn checked_range_with_limit(offset: usize, len: usize, limit: usize) -> Result<()> {
    offset
        .checked_add(len)
        .filter(|end| *end <= limit)
        .map(|_| ())
        .ok_or_else(|| L5mError::Format("segment section offset out of bounds".to_string()))
}

fn read_array32(bytes: &[u8], offset: usize) -> Result<[u8; 32]> {
    checked_range(bytes, offset, 32)?;
    Ok(bytes[offset..offset + 32].try_into().expect("slice length"))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    checked_range(bytes, offset, 4)?;
    Ok(u32::from_le_bytes(
        bytes[offset..offset + 4].try_into().expect("slice length"),
    ))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64> {
    checked_range(bytes, offset, 8)?;
    Ok(u64::from_le_bytes(
        bytes[offset..offset + 8].try_into().expect("slice length"),
    ))
}

fn read_i64(bytes: &[u8], offset: usize) -> Result<i64> {
    checked_range(bytes, offset, 8)?;
    Ok(i64::from_le_bytes(
        bytes[offset..offset + 8].try_into().expect("slice length"),
    ))
}

fn read_i16(bytes: &[u8], offset: usize) -> Result<i16> {
    checked_range(bytes, offset, 2)?;
    Ok(i16::from_le_bytes(
        bytes[offset..offset + 2].try_into().expect("slice length"),
    ))
}

fn read_u128(bytes: &[u8], offset: usize) -> Result<u128> {
    checked_range(bytes, offset, 16)?;
    Ok(u128::from_le_bytes(
        bytes[offset..offset + 16].try_into().expect("slice length"),
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::compiler::{compile_segment, CompileOptions};

    fn tiny_segment() -> Result<(tempfile::TempDir, std::path::PathBuf)> {
        let dir = tempdir()?;
        let input = dir.path().join("input.json");
        let output = dir.path().join("test.segment");
        fs::write(
            &input,
            r#"[
              {
                "capsule_id": "1",
                "tenant_id": 1,
                "claim": "Production backups are retained for 35 days.",
                "evidence": "Approved backup policy.",
                "source_id": 10,
                "valid_from": 1,
                "observed_at": 1,
                "last_verified_at": 1,
                "context_mask": "0x1",
                "policy_mask": "0xffff",
                "trust_level": 8,
                "classification": 1,
                "poison_risk": 0
              }
            ]"#,
        )?;
        compile_segment(CompileOptions {
            input_json: input,
            output_segment: output.clone(),
            epoch: 1,
        })?;
        Ok((dir, output))
    }

    fn write_segment_hash(bytes: &mut [u8]) {
        bytes[HASH_OFFSET..HASH_OFFSET + HASH_LEN].fill(0);
        let hash = segment_hash(bytes);
        bytes[HASH_OFFSET..HASH_OFFSET + HASH_LEN].copy_from_slice(hash.as_bytes());
    }

    #[test]
    fn loader_rejects_content_hash_mismatch_even_when_segment_hash_matches() -> Result<()> {
        let (_dir, output) = tiny_segment()?;
        let mut bytes = fs::read(&output)?;
        bytes[HEADER_LEN + 300] ^= 0xff;
        write_segment_hash(&mut bytes);
        fs::write(&output, bytes)?;

        let err = match Segment::open(output) {
            Ok(_) => panic!("content hash mismatch should be rejected"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("content hash mismatch"));
        Ok(())
    }

    #[test]
    fn loader_rejects_non_monotonic_section_offsets() -> Result<()> {
        let (_dir, output) = tiny_segment()?;
        let mut bytes = fs::read(&output)?;
        let string_offset = read_u64(&bytes, 48)?;
        bytes[56..64].copy_from_slice(&(string_offset - 1).to_le_bytes());
        write_segment_hash(&mut bytes);
        fs::write(&output, bytes)?;

        let err = match Segment::open(output) {
            Ok(_) => panic!("non-monotonic offsets should be rejected"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("section offsets"));
        Ok(())
    }
}
