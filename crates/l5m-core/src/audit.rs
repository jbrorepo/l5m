//! Tamper-evident access audit trail.
//!
//! Every retrieval can emit an [`AuditRecord`] capturing **who asked** (the
//! principal context on the probe), **what the gates did** (candidate counts),
//! and **what was disclosed** (returned capsule ids + their source hashes). The
//! records are **hash-chained**: each record's hash covers the previous record's
//! hash, so deleting, reordering, or editing any entry breaks the chain and is
//! detectable by [`verify_chain`]. This is the forensic/compliance evidence an
//! enterprise needs to answer "did the AI ever surface X to Y, and was it
//! allowed?".

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{frame::MemoryFrame, probe::MemoryProbe, L5mError, Result};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AuditReturned {
    pub capsule_id: String,
    pub source_hash: String,
    pub trust_level: u8,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AuditRecord {
    pub timestamp_unix: u64,
    // Principal context the query ran under (the access decision inputs).
    pub tenant_id: u64,
    pub context_mask: String,
    pub policy_mask: String,
    pub trust_floor: u8,
    pub as_of: i64,
    // The query is recorded as a hash, not plaintext, to avoid logging sensitive
    // query content while preserving correlation/forensics.
    pub query_hash: String,
    // What the gates did and what was disclosed.
    pub candidate_count_before_scoring: usize,
    pub returned_count: usize,
    pub returned: Vec<AuditReturned>,
    // Hash chain.
    pub prev_hash: String,
    pub record_hash: String,
}

impl AuditRecord {
    /// Build a record (without chaining) from a probe and the frame it produced.
    pub fn from_query(probe: &MemoryProbe, frame: &MemoryFrame, timestamp_unix: u64) -> Self {
        let returned = frame
            .capsules
            .iter()
            .map(|c| AuditReturned {
                capsule_id: c.capsule_id.to_string(),
                source_hash: hex32(&c.source_hash),
                trust_level: c.trust_level,
            })
            .collect::<Vec<_>>();
        AuditRecord {
            timestamp_unix,
            tenant_id: probe.tenant_id,
            context_mask: format!("{:#x}", probe.context_mask),
            policy_mask: format!("{:#x}", probe.caller_policy_mask),
            trust_floor: probe.trust_floor,
            as_of: probe.as_of,
            query_hash: hex32(blake3::hash(probe.query_text.as_bytes()).as_bytes()),
            candidate_count_before_scoring: frame.coverage.candidate_count_before_scoring,
            returned_count: frame.capsules.len(),
            returned,
            prev_hash: String::new(),
            record_hash: String::new(),
        }
    }

    /// Deterministic content hash over `prev_hash` + every field except
    /// `record_hash`. Any change to any field (or the chain) changes this.
    fn compute_hash(&self) -> String {
        let mut h = blake3::Hasher::new();
        h.update(self.prev_hash.as_bytes());
        h.update(&self.timestamp_unix.to_le_bytes());
        h.update(&self.tenant_id.to_le_bytes());
        h.update(self.context_mask.as_bytes());
        h.update(self.policy_mask.as_bytes());
        h.update(&[self.trust_floor]);
        h.update(&self.as_of.to_le_bytes());
        h.update(self.query_hash.as_bytes());
        h.update(&(self.candidate_count_before_scoring as u64).to_le_bytes());
        h.update(&(self.returned_count as u64).to_le_bytes());
        for r in &self.returned {
            h.update(r.capsule_id.as_bytes());
            h.update(r.source_hash.as_bytes());
            h.update(&[r.trust_level]);
        }
        hex32(h.finalize().as_bytes())
    }
}

/// Append-only, hash-chained audit log backed by a JSONL file.
pub struct AuditLog {
    file: File,
    prev_hash: String,
}

impl AuditLog {
    /// Open (creating if needed) an audit log, resuming the hash chain from the
    /// last existing record so appends remain verifiable across restarts.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let prev_hash = last_record_hash(path.as_ref())?;
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self { file, prev_hash })
    }

    /// Record a query and its result, returning the written record.
    pub fn record(
        &mut self,
        probe: &MemoryProbe,
        frame: &MemoryFrame,
        timestamp_unix: u64,
    ) -> Result<AuditRecord> {
        let mut record = AuditRecord::from_query(probe, frame, timestamp_unix);
        record.prev_hash = self.prev_hash.clone();
        record.record_hash = record.compute_hash();
        let line = serde_json::to_string(&record)?;
        self.file.write_all(line.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.file.flush()?;
        self.prev_hash = record.record_hash.clone();
        Ok(record)
    }
}

const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn last_record_hash(path: &Path) -> Result<String> {
    if !path.exists() {
        return Ok(ZERO_HASH.to_string());
    }
    let mut last = ZERO_HASH.to_string();
    for line in BufReader::new(File::open(path)?).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let record: AuditRecord = serde_json::from_str(&line)?;
        last = record.record_hash;
    }
    Ok(last)
}

/// Verify the integrity of an audit log: every record's hash must recompute, and
/// each record's `prev_hash` must equal the previous record's `record_hash`.
/// Returns the number of verified records, or an error naming the broken index.
pub fn verify_chain(path: impl AsRef<Path>) -> Result<usize> {
    let mut expected_prev = ZERO_HASH.to_string();
    let mut count = 0usize;
    for (index, line) in BufReader::new(File::open(path)?).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let record: AuditRecord = serde_json::from_str(&line)
            .map_err(|err| L5mError::Format(format!("audit record {index} unparseable: {err}")))?;
        if record.prev_hash != expected_prev {
            return Err(L5mError::Format(format!(
                "audit chain broken at record {index}: prev_hash mismatch"
            )));
        }
        if record.compute_hash() != record.record_hash {
            return Err(L5mError::Format(format!(
                "audit chain broken at record {index}: record was modified"
            )));
        }
        expected_prev = record.record_hash;
        count += 1;
    }
    Ok(count)
}

fn hex32(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}
