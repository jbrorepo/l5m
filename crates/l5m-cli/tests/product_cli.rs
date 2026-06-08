use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};

use tempfile::tempdir;

#[test]
fn cli_request_query_returns_query_response() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("capsules.json");
    let segment = dir.path().join("test.segment");
    let request = dir.path().join("query.json");
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
    )
    .unwrap();
    assert!(Command::new(env!("CARGO_BIN_EXE_l5m"))
        .args([
            "compile",
            "--input",
            input.to_str().unwrap(),
            "--output",
            segment.to_str().unwrap(),
            "--epoch",
            "1",
        ])
        .status()
        .unwrap()
        .success());
    fs::write(
        &request,
        r#"{"query":"How long are production backups retained?","tenant_id":1,"as_of":10,"context_mask":"0x1","policy_mask":"0xffff","trust_floor":4,"max_capsules":8,"max_tokens":1024,"mode":"L5m"}"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_l5m"))
        .args([
            "query",
            "--segment",
            segment.to_str().unwrap(),
            "--request",
            request.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"frame\""));
    assert!(stdout.contains("\"mode\""));
}

#[test]
fn cli_serve_stdio_handles_one_query_line() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("capsules.json");
    let segment = dir.path().join("test.segment");
    fs::write(
        &input,
        r#"[{"capsule_id":"1","tenant_id":1,"claim":"Production deploy freeze starts Friday.","evidence":"Release calendar.","source_id":10,"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0x1","policy_mask":"0xffff","trust_level":8,"classification":1,"poison_risk":0}]"#,
    )
    .unwrap();
    assert!(Command::new(env!("CARGO_BIN_EXE_l5m"))
        .args([
            "compile",
            "--input",
            input.to_str().unwrap(),
            "--output",
            segment.to_str().unwrap(),
            "--epoch",
            "1",
        ])
        .status()
        .unwrap()
        .success());
    let mut child = Command::new(env!("CARGO_BIN_EXE_l5m"))
        .args(["serve-stdio", "--segment", segment.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(stdin, "{{\"query\":\"When is deploy freeze?\",\"tenant_id\":1,\"as_of\":10,\"context_mask\":\"0x1\",\"policy_mask\":\"0xffff\",\"trust_floor\":4,\"max_capsules\":8,\"max_tokens\":1024,\"mode\":\"L5m\"}}").unwrap();
    }
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(stdout.lines().any(|line| line.contains("\"frame\"")));
}
