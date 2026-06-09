// F4: JWKS provider tests — kid-based key selection and hot rotation.
//
// RSA keypairs are generated at test runtime (rsa crate, dev-dependency only);
// no private-key material is committed to the repository.

use std::io::Write;

use axum::http::HeaderMap;
use jsonwebtoken::{encode, EncodingKey, Header};
use l5m_server::principal::PrincipalProvider;
use l5m_server::{JwksPrincipalProvider, Scope};
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::traits::PublicKeyParts;
use rsa::RsaPrivateKey;

/// A freshly generated signing key plus its public JWK document entry.
struct TestKey {
    pem: String,
    jwk: serde_json::Value,
}

fn generate_key(kid: &str) -> TestKey {
    let mut rng = rand::thread_rng();
    let key = RsaPrivateKey::new(&mut rng, 2048).expect("keygen");
    let pem = key
        .to_pkcs1_pem(rsa::pkcs1::LineEnding::LF)
        .expect("pem")
        .to_string();
    let jwk = serde_json::json!({
        "kty": "RSA",
        "kid": kid,
        "use": "sig",
        "alg": "RS256",
        "n": b64url(&key.n().to_bytes_be()),
        "e": b64url(&key.e().to_bytes_be()),
    });
    TestKey { pem, jwk }
}

/// Minimal base64url (no padding) — avoids a base64 dev-dependency.
fn b64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[n as usize & 63] as char);
        }
    }
    out
}

fn write_jwks(path: &std::path::Path, keys: &[&TestKey]) {
    let doc = serde_json::json!({
        "keys": keys.iter().map(|k| k.jwk.clone()).collect::<Vec<_>>()
    });
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(doc.to_string().as_bytes()).unwrap();
    f.flush().unwrap();
}

fn sign(key: &TestKey, kid: &str, claims: serde_json::Value) -> String {
    let mut header = Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some(kid.to_string());
    encode(
        &header,
        &claims,
        &EncodingKey::from_rsa_pem(key.pem.as_bytes()).unwrap(),
    )
    .unwrap()
}

fn bearer(token: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("authorization", format!("Bearer {token}").parse().unwrap());
    h
}

fn future_exp() -> usize {
    (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600) as usize
}

#[test]
fn jwks_verifies_by_kid_and_rejects_unknown_keys() {
    let dir = tempfile::tempdir().unwrap();
    let jwks_path = dir.path().join("jwks.json");
    let key1 = generate_key("key-1");
    let rogue = generate_key("rogue");
    write_jwks(&jwks_path, &[&key1]);

    let provider = JwksPrincipalProvider::from_file(&jwks_path).unwrap();

    // Token signed by the published key verifies; claims drive the principal.
    let token = sign(
        &key1,
        "key-1",
        serde_json::json!({"tenant": 9, "scope": "read", "exp": future_exp()}),
    );
    let p = provider.principal(&bearer(&token)).unwrap();
    assert_eq!(p.tenant_id, 9);
    assert_eq!(p.scope, Scope::Read);

    // Token signed by a key NOT in the JWKS is rejected even with a known kid.
    let forged = sign(
        &rogue,
        "key-1",
        serde_json::json!({"tenant": 1, "exp": future_exp()}),
    );
    assert!(provider.principal(&bearer(&forged)).is_err());
}

#[test]
fn jwks_rotation_is_picked_up_without_restart() {
    let dir = tempfile::tempdir().unwrap();
    let jwks_path = dir.path().join("jwks.json");
    let key1 = generate_key("key-1");
    let key2 = generate_key("key-2");
    write_jwks(&jwks_path, &[&key1]);

    let provider = JwksPrincipalProvider::from_file(&jwks_path).unwrap();
    let token1 = sign(
        &key1,
        "key-1",
        serde_json::json!({"tenant": 1, "exp": future_exp()}),
    );
    let token2 = sign(
        &key2,
        "key-2",
        serde_json::json!({"tenant": 2, "exp": future_exp()}),
    );

    assert!(provider.principal(&bearer(&token1)).is_ok());
    assert!(
        provider.principal(&bearer(&token2)).is_err(),
        "key-2 not yet published"
    );

    // Rotate: publish key-2, retire key-1 — same provider, no restart.
    // (Ensure the mtime moves even on coarse filesystem clocks.)
    std::thread::sleep(std::time::Duration::from_millis(1100));
    write_jwks(&jwks_path, &[&key2]);

    assert!(
        provider.principal(&bearer(&token2)).is_ok(),
        "newly published key accepted after reload"
    );
    assert!(
        provider.principal(&bearer(&token1)).is_err(),
        "retired key rejected after reload"
    );
}
