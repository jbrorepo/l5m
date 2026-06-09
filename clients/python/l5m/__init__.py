"""L5M Python client — a thin, dependency-free SDK for the L5M HTTP server.

L5M enforces security gates (tenant, context, policy, temporal, trust) *before*
relevance scoring, so an unauthorized memory is never even a candidate. This
client never sends the principal in the request body — identity travels in the
auth headers / bearer token, exactly as the server expects.

Example
-------
    from l5m import Client

    # API-key + header auth (dev / trusted network):
    c = Client("http://localhost:8080", api_key="secret", tenant_id=7)
    c.insert({
        "capsule_id": "1", "claim": "the launch is in March",
        "evidence": "the launch is in March",
        "source_id": 1, "valid_from": 1, "observed_at": 1, "last_verified_at": 1,
        "context_mask": "0xffff", "policy_mask": "0xffff",
        "trust_level": 8, "classification": 1, "poison_risk": 0,
    })
    frame = c.query("when is the launch?")
    for cap in frame["frame"]["capsules"]:
        print(cap["score"], cap["claim"])

    # JWT bearer auth (production): the tenant/policy/trust come from verified
    # claims, so you don't pass tenant_id.
    c = Client("https://l5m.internal", bearer_token=my_jwt)
"""

from .client import Client, L5mError, AuthError, RateLimited

__all__ = ["Client", "L5mError", "AuthError", "RateLimited"]
__version__ = "0.1.0"
