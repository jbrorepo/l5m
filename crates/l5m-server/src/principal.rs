//! Principal resolution — the authentication boundary.
//!
//! L5M *enforces* the gates for whatever principal it is given; it does not
//! authenticate the caller. A [`PrincipalProvider`] maps an incoming request to
//! the tenant/context/policy/trust the gates run under. The bundled
//! [`HeaderPrincipalProvider`] is for development and trusted-network deployments
//! (an API key + `X-L5M-*` headers). **In production, implement this trait
//! against your IdP** (verify a JWT/OIDC token and derive the principal from its
//! claims) so a client can never assert a tenant it isn't entitled to.

use axum::http::HeaderMap;

/// What a credential is allowed to do. Ordered: `Admin` > `Write` > `Read` —
/// a credential authorizes any operation at or below its level.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Scope {
    Read,
    Write,
    Admin,
}

impl Scope {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "read" | "ro" => Some(Self::Read),
            "write" | "rw" => Some(Self::Write),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Principal {
    pub tenant_id: u64,
    pub context_mask: String,
    pub policy_mask: String,
    pub trust_floor: u8,
    /// Operations this principal's credential authorizes.
    pub scope: Scope,
}

#[derive(Debug)]
pub enum AuthError {
    Unauthorized,
    Missing(&'static str),
    Invalid(&'static str),
}

pub trait PrincipalProvider: Send + Sync {
    fn principal(&self, headers: &HeaderMap) -> Result<Principal, AuthError>;
}

/// One API key with the operations it authorizes and (optionally) the single
/// tenant it is bound to. A tenant-bound key cannot act as any other tenant —
/// the binding wins over whatever `X-L5M-Tenant` claims.
#[derive(Clone, Debug)]
pub struct ScopedKey {
    pub secret: String,
    pub scope: Scope,
    pub tenant: Option<u64>,
}

impl ScopedKey {
    /// Parse `secret:scope[:tenant]`, e.g. `k1:read`, `k2:write:7`, `k3:admin`.
    pub fn parse(entry: &str) -> Result<Self, String> {
        let mut parts = entry.trim().splitn(3, ':');
        let secret = parts
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("empty key in {entry:?}"))?
            .to_string();
        let scope = match parts.next() {
            Some(s) => Scope::parse(s)
                .ok_or_else(|| format!("bad scope in {entry:?} (expected read|write|admin)"))?,
            None => Scope::Write,
        };
        let tenant = match parts.next() {
            Some(t) => Some(
                t.trim()
                    .parse::<u64>()
                    .map_err(|_| format!("bad tenant in {entry:?}"))?,
            ),
            None => None,
        };
        Ok(Self {
            secret,
            scope,
            tenant,
        })
    }
}

/// API-key + header based principal resolution. Each accepted key carries a
/// [`Scope`] (read/write/admin) and may be bound to a single tenant. The rest
/// of the principal is read from `X-L5M-Tenant` (required unless the key binds
/// one), `X-L5M-Context`/`X-L5M-Policy` (hex masks, default `0xffff`), and
/// `X-L5M-Trust` (default `0`).
///
/// With no keys configured, requests are accepted at `Write` scope — the
/// open development mode (do not expose to untrusted networks).
pub struct HeaderPrincipalProvider {
    keys: Vec<ScopedKey>,
}

impl HeaderPrincipalProvider {
    /// No authentication (development / trusted networks only).
    pub fn open() -> Self {
        Self { keys: Vec::new() }
    }

    /// A single full-access (write-scope) key — the pre-scoped behavior.
    pub fn single(api_key: impl Into<String>) -> Self {
        Self {
            keys: vec![ScopedKey {
                secret: api_key.into(),
                scope: Scope::Write,
                tenant: None,
            }],
        }
    }

    pub fn with_keys(keys: Vec<ScopedKey>) -> Self {
        Self { keys }
    }

    /// Compatibility constructor matching the old `{ api_key: Option<String> }`
    /// shape: `None` = open, `Some(k)` = one write-scope key.
    pub fn from_optional_key(api_key: Option<String>) -> Self {
        match api_key {
            Some(key) => Self::single(key),
            None => Self::open(),
        }
    }
}

impl PrincipalProvider for HeaderPrincipalProvider {
    fn principal(&self, headers: &HeaderMap) -> Result<Principal, AuthError> {
        let matched: Option<&ScopedKey> = if self.keys.is_empty() {
            None // open mode
        } else {
            let presented = headers
                .get("x-l5m-api-key")
                .and_then(|v| v.to_str().ok())
                .ok_or(AuthError::Unauthorized)?;
            Some(
                self.keys
                    .iter()
                    .find(|k| k.secret == presented)
                    .ok_or(AuthError::Unauthorized)?,
            )
        };
        let scope = matched.map_or(Scope::Write, |k| k.scope);

        // A tenant-bound key forces its tenant; otherwise the header names it.
        let tenant_id = match matched.and_then(|k| k.tenant) {
            Some(bound) => bound,
            None => header(headers, "x-l5m-tenant")
                .ok_or(AuthError::Missing("X-L5M-Tenant"))?
                .parse::<u64>()
                .map_err(|_| AuthError::Invalid("X-L5M-Tenant"))?,
        };
        let context_mask = header(headers, "x-l5m-context")
            .unwrap_or("0xffff")
            .to_string();
        let policy_mask = header(headers, "x-l5m-policy")
            .unwrap_or("0xffff")
            .to_string();
        let trust_floor = header(headers, "x-l5m-trust")
            .unwrap_or("0")
            .parse::<u8>()
            .map_err(|_| AuthError::Invalid("X-L5M-Trust"))?;
        Ok(Principal {
            tenant_id,
            context_mask,
            policy_mask,
            trust_floor,
            scope,
        })
    }
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

// ---------------------------------------------------------------------------
// Production provider: derive the principal from a verified JWT.
// ---------------------------------------------------------------------------

use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct L5mClaims {
    /// Tenant id (required). Everything else falls back to a safe default.
    tenant: u64,
    #[serde(default)]
    context: Option<String>,
    #[serde(default)]
    policy: Option<String>,
    #[serde(default)]
    trust: Option<u8>,
    /// Optional `scope` claim: `read` | `write` | `admin` (default write).
    #[serde(default)]
    scope: Option<String>,
}

impl L5mClaims {
    fn into_principal(self) -> Principal {
        let scope = self
            .scope
            .as_deref()
            .and_then(Scope::parse)
            .unwrap_or(Scope::Write);
        Principal {
            tenant_id: self.tenant,
            context_mask: self.context.unwrap_or_else(|| "0xffff".to_string()),
            policy_mask: self.policy.unwrap_or_else(|| "0xffff".to_string()),
            trust_floor: self.trust.unwrap_or(0),
            scope,
        }
    }
}

/// Resolve the principal from a verified `Authorization: Bearer <jwt>`. The
/// signature and expiry are checked cryptographically, so — unlike header-based
/// auth — a client cannot assert a tenant it isn't entitled to. Map your IdP's
/// claims onto `tenant`/`context`/`policy`/`trust` (e.g. via a token exchange).
pub struct JwtPrincipalProvider {
    decoding_key: DecodingKey,
    validation: Validation,
}

impl JwtPrincipalProvider {
    /// HMAC-SHA256 with a shared secret.
    pub fn hs256(secret: &[u8]) -> Self {
        Self {
            decoding_key: DecodingKey::from_secret(secret),
            validation: Validation::new(Algorithm::HS256),
        }
    }

    /// RS256 with an RSA public key in PEM form (typical for OIDC providers).
    pub fn rs256_pem(pem: &[u8]) -> Result<Self, jsonwebtoken::errors::Error> {
        Ok(Self {
            decoding_key: DecodingKey::from_rsa_pem(pem)?,
            validation: Validation::new(Algorithm::RS256),
        })
    }

    /// Restrict accepted audiences (recommended).
    pub fn with_audience(mut self, audiences: &[String]) -> Self {
        self.validation.set_audience(audiences);
        self
    }
}

impl PrincipalProvider for JwtPrincipalProvider {
    fn principal(&self, headers: &HeaderMap) -> Result<Principal, AuthError> {
        let token = bearer_token(headers)?;
        let data = decode::<L5mClaims>(token, &self.decoding_key, &self.validation)
            .map_err(|_| AuthError::Unauthorized)?;
        Ok(data.claims.into_principal())
    }
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, AuthError> {
    header(headers, "authorization")
        .and_then(|v| {
            v.strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))
        })
        .ok_or(AuthError::Missing("Authorization: Bearer"))
}

// ---------------------------------------------------------------------------
// JWKS provider: multiple RS256 keys selected by `kid`, hot-reloaded from a
// JSON Web Key Set file — key rotation without a restart.
// ---------------------------------------------------------------------------

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

/// Resolve the principal from a verified RS256 bearer token, with the public
/// keys loaded from a standard JWKS document (`{"keys":[{kty,kid,n,e},…]}`) on
/// disk. The file's mtime is checked per request and the key set is reloaded
/// when it changes, so rotating keys is: write the new JWKS file. (Fetching
/// the JWKS from your IdP is the deployer's job — a sidecar/cron `curl` —
/// which keeps an HTTP client out of this binary's supply chain.)
pub struct JwksPrincipalProvider {
    path: PathBuf,
    validation: Validation,
    cache: Mutex<JwksCache>,
}

struct JwksCache {
    loaded_at: Option<SystemTime>,
    keys: std::collections::HashMap<String, DecodingKey>,
}

#[derive(Deserialize)]
struct JwksDoc {
    keys: Vec<JwkEntry>,
}

#[derive(Deserialize)]
struct JwkEntry {
    #[serde(default)]
    kty: String,
    #[serde(default)]
    kid: Option<String>,
    #[serde(default)]
    n: Option<String>,
    #[serde(default)]
    e: Option<String>,
}

impl JwksPrincipalProvider {
    pub fn from_file(path: impl Into<PathBuf>) -> Result<Self, String> {
        let provider = Self {
            path: path.into(),
            validation: Validation::new(Algorithm::RS256),
            cache: Mutex::new(JwksCache {
                loaded_at: None,
                keys: std::collections::HashMap::new(),
            }),
        };
        provider.reload()?; // fail fast on a bad document at startup
        Ok(provider)
    }

    /// Restrict accepted audiences (recommended).
    pub fn with_audience(mut self, audiences: &[String]) -> Self {
        self.validation.set_audience(audiences);
        self
    }

    fn reload(&self) -> Result<(), String> {
        let mtime = std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .map_err(|e| format!("stat {:?}: {e}", self.path))?;
        let raw = std::fs::read_to_string(&self.path)
            .map_err(|e| format!("read {:?}: {e}", self.path))?;
        let doc: JwksDoc =
            serde_json::from_str(&raw).map_err(|e| format!("parse {:?}: {e}", self.path))?;
        let mut keys = std::collections::HashMap::new();
        for (i, jwk) in doc.keys.iter().enumerate() {
            if jwk.kty != "RSA" {
                continue; // only RS256 supported here
            }
            let (Some(n), Some(e)) = (&jwk.n, &jwk.e) else {
                continue;
            };
            let key = DecodingKey::from_rsa_components(n, e)
                .map_err(|err| format!("jwk #{i} in {:?}: {err}", self.path))?;
            let kid = jwk.kid.clone().unwrap_or_else(|| format!("__nokid_{i}"));
            keys.insert(kid, key);
        }
        if keys.is_empty() {
            return Err(format!("no usable RSA keys in {:?}", self.path));
        }
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.loaded_at = Some(mtime);
        cache.keys = keys;
        Ok(())
    }

    fn reload_if_changed(&self) {
        let current = std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .ok();
        let cached = {
            let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            cache.loaded_at
        };
        if current.is_some() && current != cached {
            // Best-effort: a malformed mid-rotation file keeps the old keys.
            let _ = self.reload();
        }
    }
}

impl PrincipalProvider for JwksPrincipalProvider {
    fn principal(&self, headers: &HeaderMap) -> Result<Principal, AuthError> {
        let token = bearer_token(headers)?;
        self.reload_if_changed();

        let kid = jsonwebtoken::decode_header(token)
            .map_err(|_| AuthError::Unauthorized)?
            .kid;
        let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        // With a kid, use exactly that key; without one, try each known key.
        let candidates: Vec<&DecodingKey> = match &kid {
            Some(kid) => cache.keys.get(kid).into_iter().collect(),
            None => cache.keys.values().collect(),
        };
        for key in candidates {
            if let Ok(data) = decode::<L5mClaims>(token, key, &self.validation) {
                return Ok(data.claims.into_principal());
            }
        }
        Err(AuthError::Unauthorized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};

    fn token(secret: &[u8], body: serde_json::Value) -> String {
        encode(&Header::default(), &body, &EncodingKey::from_secret(secret)).unwrap()
    }

    fn bearer(token: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("authorization", format!("Bearer {token}").parse().unwrap());
        h
    }

    #[test]
    fn jwt_principal_is_derived_from_verified_claims() {
        let secret = b"super-secret-key";
        let provider = JwtPrincipalProvider::hs256(secret);
        let exp = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600) as usize;
        let t = token(
            secret,
            serde_json::json!({"tenant": 7, "policy": "0x2", "trust": 5, "exp": exp}),
        );
        let p = provider.principal(&bearer(&t)).unwrap();
        assert_eq!(p.tenant_id, 7);
        assert_eq!(p.policy_mask, "0x2");
        assert_eq!(p.trust_floor, 5);
    }

    #[test]
    fn jwt_with_wrong_signature_is_rejected() {
        let provider = JwtPrincipalProvider::hs256(b"the-real-secret");
        let exp = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600) as usize;
        let forged = token(
            b"attacker-secret",
            serde_json::json!({"tenant": 1, "exp": exp}),
        );
        assert!(matches!(
            provider.principal(&bearer(&forged)),
            Err(AuthError::Unauthorized)
        ));
    }

    #[test]
    fn expired_jwt_is_rejected() {
        let secret = b"k";
        let provider = JwtPrincipalProvider::hs256(secret);
        let t = token(secret, serde_json::json!({"tenant": 1, "exp": 1_000usize}));
        assert!(provider.principal(&bearer(&t)).is_err());
    }
}
