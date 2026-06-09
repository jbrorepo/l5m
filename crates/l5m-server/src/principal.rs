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

#[derive(Clone, Debug)]
pub struct Principal {
    pub tenant_id: u64,
    pub context_mask: String,
    pub policy_mask: String,
    pub trust_floor: u8,
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

/// API-key + header based principal resolution. If `api_key` is set, requests
/// must present a matching `X-L5M-Api-Key`. The principal is read from
/// `X-L5M-Tenant` (required), `X-L5M-Context`/`X-L5M-Policy` (hex masks, default
/// `0xffff`), and `X-L5M-Trust` (default `0`).
pub struct HeaderPrincipalProvider {
    pub api_key: Option<String>,
}

impl PrincipalProvider for HeaderPrincipalProvider {
    fn principal(&self, headers: &HeaderMap) -> Result<Principal, AuthError> {
        if let Some(expected) = &self.api_key {
            let presented = headers.get("x-l5m-api-key").and_then(|v| v.to_str().ok());
            if presented != Some(expected.as_str()) {
                return Err(AuthError::Unauthorized);
            }
        }
        let tenant_id = header(headers, "x-l5m-tenant")
            .ok_or(AuthError::Missing("X-L5M-Tenant"))?
            .parse::<u64>()
            .map_err(|_| AuthError::Invalid("X-L5M-Tenant"))?;
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
        let token = header(headers, "authorization")
            .and_then(|v| {
                v.strip_prefix("Bearer ")
                    .or_else(|| v.strip_prefix("bearer "))
            })
            .ok_or(AuthError::Missing("Authorization: Bearer"))?;
        let data = decode::<L5mClaims>(token, &self.decoding_key, &self.validation)
            .map_err(|_| AuthError::Unauthorized)?;
        let c = data.claims;
        Ok(Principal {
            tenant_id: c.tenant,
            context_mask: c.context.unwrap_or_else(|| "0xffff".to_string()),
            policy_mask: c.policy.unwrap_or_else(|| "0xffff".to_string()),
            trust_floor: c.trust.unwrap_or(0),
        })
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
