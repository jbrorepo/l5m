#![cfg(feature = "encryption")]
// E1: encryption at rest. Sealed segments must round-trip, reject the wrong key,
// detect tampering, and never be mistaken for plaintext.

use std::fs;
use std::path::PathBuf;

use l5m_core::compiler::compile_segment_sealed;
use l5m_core::crypto::{EnvKey, StaticKey};
use l5m_core::{retrieve, CompileOptions, MemoryProbe, Result, Segment};
use tempfile::tempdir;

const CORPUS: &str = r#"[
  {"capsule_id":"1","tenant_id":1,"claim":"the violet passphrase is kelpstone","evidence":"the violet passphrase is kelpstone","source_id":1,"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,"classification":1,"poison_risk":0}
]"#;

fn test_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    for (i, b) in key.iter_mut().enumerate() {
        *b = i as u8;
    }
    key
}

fn seal_corpus(dir: &std::path::Path, key: &dyn l5m_core::crypto::KeyProvider) -> Result<PathBuf> {
    let input = dir.join("in.json");
    let output = dir.join("sealed.segment");
    fs::write(&input, CORPUS)?;
    compile_segment_sealed(
        CompileOptions {
            input_json: input,
            output_segment: output.clone(),
            epoch: 1,
        },
        key,
    )?;
    Ok(output)
}

fn finds_passphrase(seg: &Segment) -> Result<bool> {
    let mut p = MemoryProbe::build("violet passphrase kelpstone", 1, 1000, 0xffff, 0xffff, 4);
    p.max_capsules = 8;
    let frame = retrieve(seg, &p)?;
    Ok(frame.capsules.iter().any(|c| c.claim.contains("kelpstone")))
}

#[test]
fn sealed_segment_round_trips() -> Result<()> {
    let dir = tempdir()?;
    let out = seal_corpus(dir.path(), &StaticKey(test_key()))?;
    // The sealed file is ciphertext: the plaintext must not be on disk.
    let on_disk = fs::read(&out)?;
    assert!(&on_disk[..8] == b"L5MSEAL1", "file should be sealed");
    assert!(
        !on_disk.windows(9).any(|w| w == b"kelpstone"),
        "plaintext leaked into the sealed file"
    );
    let seg = Segment::open_sealed(&out, &test_key())?;
    assert!(finds_passphrase(&seg)?, "should decrypt and retrieve");
    Ok(())
}

#[test]
fn wrong_key_is_rejected() -> Result<()> {
    let dir = tempdir()?;
    let out = seal_corpus(dir.path(), &StaticKey(test_key()))?;
    let mut bad = test_key();
    bad[0] ^= 0xff;
    assert!(
        Segment::open_sealed(&out, &bad).is_err(),
        "wrong key must fail"
    );
    Ok(())
}

#[test]
fn tampering_is_detected() -> Result<()> {
    let dir = tempdir()?;
    let out = seal_corpus(dir.path(), &StaticKey(test_key()))?;
    let mut bytes = fs::read(&out)?;
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff; // flip a ciphertext/tag byte
    fs::write(&out, &bytes)?;
    assert!(
        Segment::open_sealed(&out, &test_key()).is_err(),
        "tampered ciphertext must fail the AEAD tag"
    );
    Ok(())
}

#[test]
fn plaintext_open_rejects_a_sealed_file() -> Result<()> {
    let dir = tempdir()?;
    let out = seal_corpus(dir.path(), &StaticKey(test_key()))?;
    assert!(
        Segment::open(&out).is_err(),
        "the plaintext loader must not accept a sealed file"
    );
    Ok(())
}

#[test]
fn env_key_provider_round_trips() -> Result<()> {
    let hex: String = test_key().iter().map(|b| format!("{b:02x}")).collect();
    std::env::set_var("L5M_TEST_DEK", &hex);
    let dir = tempdir()?;
    let out = seal_corpus(dir.path(), &EnvKey("L5M_TEST_DEK".to_string()))?;
    let seg = Segment::open_sealed(&out, &test_key())?;
    assert!(finds_passphrase(&seg)?);
    Ok(())
}
