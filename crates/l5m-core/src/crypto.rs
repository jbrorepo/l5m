//! Encryption at rest for compiled segments (the `encryption` feature).
//!
//! A *sealed* segment is an authenticated-encryption (AEAD, ChaCha20-Poly1305)
//! wrapper around a normal compiled segment:
//!
//! ```text
//! "L5MSEAL1" (8)  ||  nonce (12)  ||  AEAD-ciphertext(plaintext_segment, aad=magic)
//! ```
//!
//! Confidentiality + integrity come from the AEAD; the segment's own BLAKE3
//! self-hash still protects the plaintext after decryption. Keys are 32 bytes
//! supplied by a [`KeyProvider`] (static, environment, or a customer KMS/Vault
//! integration), so L5M never owns long-term key material.

use std::path::Path;

use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Key, Nonce,
};

use crate::{L5mError, Result};

const SEAL_MAGIC: &[u8; 8] = b"L5MSEAL1";
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

/// Supplies the 32-byte data-encryption key. Implement this against a KMS/Vault
/// to keep keys out of the process and enable rotation/envelope encryption.
pub trait KeyProvider {
    fn key(&self) -> Result<[u8; 32]>;
}

/// A key held directly in memory (e.g. an envelope key just unwrapped by a KMS).
pub struct StaticKey(pub [u8; 32]);
impl KeyProvider for StaticKey {
    fn key(&self) -> Result<[u8; 32]> {
        Ok(self.0)
    }
}

/// A key read from an environment variable as 64 hex chars (32 bytes).
pub struct EnvKey(pub String);
impl KeyProvider for EnvKey {
    fn key(&self) -> Result<[u8; 32]> {
        let hex = std::env::var(&self.0)
            .map_err(|_| L5mError::Format(format!("key env var {} not set", self.0)))?;
        decode_hex_key(hex.trim())
    }
}

fn decode_hex_key(hex: &str) -> Result<[u8; 32]> {
    if hex.len() != 64 {
        return Err(L5mError::Format(
            "key must be 64 hex characters (32 bytes)".to_string(),
        ));
    }
    let mut key = [0u8; 32];
    for (index, byte) in key.iter_mut().enumerate() {
        let hi = hex_val(hex.as_bytes()[index * 2])?;
        let lo = hex_val(hex.as_bytes()[index * 2 + 1])?;
        *byte = (hi << 4) | lo;
    }
    Ok(key)
}

fn hex_val(c: u8) -> Result<u8> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(L5mError::Format("invalid hex character in key".to_string())),
    }
}

/// AEAD-encrypt `plaintext` into the sealed wire format.
pub fn seal(plaintext: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let mut nonce_bytes = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce_bytes)
        .map_err(|err| L5mError::Format(format!("nonce generation failed: {err}")))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad: SEAL_MAGIC,
            },
        )
        .map_err(|_| L5mError::Format("seal (encrypt) failed".to_string()))?;
    let mut out = Vec::with_capacity(SEAL_MAGIC.len() + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(SEAL_MAGIC);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// AEAD-decrypt the sealed wire format. Fails (without leaking why) on a wrong
/// key or any tampering — the auth tag covers magic + nonce + ciphertext.
pub fn unseal(sealed: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    let header = SEAL_MAGIC.len() + NONCE_LEN;
    if sealed.len() < header + TAG_LEN || &sealed[..SEAL_MAGIC.len()] != SEAL_MAGIC {
        return Err(L5mError::Format("not a sealed L5M segment".to_string()));
    }
    let nonce = Nonce::from_slice(&sealed[SEAL_MAGIC.len()..header]);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    cipher
        .decrypt(
            nonce,
            Payload {
                msg: &sealed[header..],
                aad: SEAL_MAGIC,
            },
        )
        .map_err(|_| {
            L5mError::Format("segment decryption failed: wrong key or tampered file".to_string())
        })
}

/// Seal an already-compiled plaintext segment file to `output`.
pub fn seal_segment_file(
    plaintext_segment: impl AsRef<Path>,
    output: impl AsRef<Path>,
    key: &dyn KeyProvider,
) -> Result<()> {
    let plaintext = std::fs::read(plaintext_segment)?;
    let sealed = seal(&plaintext, &key.key()?)?;
    if let Some(parent) = output.as_ref().parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output, sealed)?;
    Ok(())
}
