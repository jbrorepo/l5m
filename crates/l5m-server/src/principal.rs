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
