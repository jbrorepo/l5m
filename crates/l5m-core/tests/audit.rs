// E2: tamper-evident access audit trail. Records must chain, capture what was
// disclosed, and any edit/removal must be detectable.

use std::fs;

use l5m_core::{
    compile_segment, retrieve, verify_audit_chain, AuditLog, CompileOptions, MemoryProbe, Result,
    Segment,
};
use tempfile::tempdir;

const CORPUS: &str = r#"[
  {"capsule_id":"1","tenant_id":1,"claim":"backup retention is 35 days","evidence":"approved policy","source_id":1,"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,"classification":1,"poison_risk":0},
  {"capsule_id":"2","tenant_id":1,"claim":"scanning cadence is weekly","evidence":"approved policy","source_id":2,"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,"classification":1,"poison_risk":0}
]"#;

fn segment() -> Result<(tempfile::TempDir, Segment)> {
    let dir = tempdir()?;
    let input = dir.path().join("in.json");
    let out = dir.path().join("seg.segment");
    fs::write(&input, CORPUS)?;
    compile_segment(CompileOptions {
        input_json: input,
        output_segment: out.clone(),
        epoch: 1,
    })?;
    Ok((dir, Segment::open(out)?))
}

fn query(seg: &Segment, q: &str) -> Result<l5m_core::MemoryFrame> {
    let mut p = MemoryProbe::build(q, 1, 1000, 0xffff, 0xffff, 4);
    p.max_capsules = 8;
    retrieve(seg, &p)
}

#[test]
fn audit_chain_records_and_verifies() -> Result<()> {
    let (dir, seg) = segment()?;
    let log_path = dir.path().join("audit.jsonl");

    let mut log = AuditLog::open(&log_path)?;
    for (i, q) in ["backup retention", "scanning cadence", "backup policy"]
        .iter()
        .enumerate()
    {
        let frame = query(&seg, q)?;
        let mut probe = MemoryProbe::build(q, 1, 1000, 0xffff, 0xffff, 4);
        probe.max_capsules = 8;
        let rec = log.record(&probe, &frame, 1_000 + i as u64)?;
        assert_eq!(rec.tenant_id, 1);
        assert!(!rec.returned.is_empty(), "should record disclosed capsules");
        // Provenance: each disclosed item carries a source hash.
        assert!(rec.returned.iter().all(|r| r.source_hash.len() == 64));
    }

    assert_eq!(verify_audit_chain(&log_path)?, 3, "all 3 records verify");
    Ok(())
}

#[test]
fn tampering_with_a_record_breaks_the_chain() -> Result<()> {
    let (dir, seg) = segment()?;
    let log_path = dir.path().join("audit.jsonl");
    let mut log = AuditLog::open(&log_path)?;
    for i in 0..3 {
        let frame = query(&seg, "backup retention")?;
        let mut probe = MemoryProbe::build("backup retention", 1, 1000, 0xffff, 0xffff, 4);
        probe.max_capsules = 8;
        log.record(&probe, &frame, 1_000 + i)?;
    }
    assert!(verify_audit_chain(&log_path).is_ok());

    // Forge the middle record: change the recorded tenant (an audit cover-up).
    let mut lines: Vec<String> = fs::read_to_string(&log_path)?
        .lines()
        .map(str::to_string)
        .collect();
    lines[1] = lines[1].replace("\"tenant_id\":1", "\"tenant_id\":2");
    fs::write(&log_path, lines.join("\n") + "\n")?;

    let err = verify_audit_chain(&log_path).unwrap_err();
    assert!(
        err.to_string().contains("audit chain broken"),
        "forgery must be detected, got: {err}"
    );
    Ok(())
}

#[test]
fn appends_resume_the_chain_across_reopen() -> Result<()> {
    let (dir, seg) = segment()?;
    let log_path = dir.path().join("audit.jsonl");
    {
        let mut log = AuditLog::open(&log_path)?;
        let frame = query(&seg, "backup retention")?;
        let mut probe = MemoryProbe::build("backup retention", 1, 1000, 0xffff, 0xffff, 4);
        probe.max_capsules = 8;
        log.record(&probe, &frame, 1)?;
    }
    // Reopen and append more — the chain must continue, not restart.
    {
        let mut log = AuditLog::open(&log_path)?;
        let frame = query(&seg, "scanning cadence")?;
        let mut probe = MemoryProbe::build("scanning cadence", 1, 1000, 0xffff, 0xffff, 4);
        probe.max_capsules = 8;
        log.record(&probe, &frame, 2)?;
    }
    assert_eq!(verify_audit_chain(&log_path)?, 2);
    Ok(())
}
