# l5m-server

A small HTTP service around the L5M engine: gated retrieval, real-time writes,
health, and Prometheus metrics — with the **principal (tenant/policy/trust)
resolved from the request, never the body**, so the security gates always run
under an authenticated identity.

## Run

```bash
# from source
L5M_API_KEY=changeme cargo run -p l5m-server

# or via Docker (Dockerfile at repo root)
docker build -t l5m-server .
docker run -p 8080:8080 -e L5M_API_KEY=changeme l5m-server
```

Environment:
- `L5M_BIND` — bind address (default `0.0.0.0:8080`)
- `L5M_SEGMENTS` — comma-separated segment paths to load (optional; empty = pure
  real-time store)
- `L5M_API_KEY` — if set, require a matching `X-L5M-Api-Key` header

## Endpoints

| Method | Path | Purpose |
|---|---|---|
| GET | `/healthz` | liveness |
| GET | `/readyz` | readiness |
| GET | `/metrics` | Prometheus metrics |
| POST | `/v1/query` | gated retrieval |
| POST | `/v1/memories` | insert/update a memory (forced to the caller's tenant) |
| DELETE | `/v1/memories/:id` | tombstone a memory |

Principal headers (dev `HeaderPrincipalProvider`): `X-L5M-Tenant` (required),
`X-L5M-Context`/`X-L5M-Policy` (hex masks, default `0xffff`), `X-L5M-Trust`
(default `0`), `X-L5M-Api-Key`.

## Example

```bash
curl -s localhost:8080/v1/memories -H 'x-l5m-api-key: changeme' \
  -H 'x-l5m-tenant: 1' -H 'content-type: application/json' -d '{
    "capsule_id":"1","claim":"backups retained 35 days","evidence":"approved policy",
    "source_id":1,"valid_from":1,"observed_at":1,"last_verified_at":1,
    "context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,
    "classification":1,"poison_risk":0 }'

curl -s localhost:8080/v1/query -H 'x-l5m-api-key: changeme' \
  -H 'x-l5m-tenant: 1' -H 'content-type: application/json' \
  -d '{"query":"how long are backups retained?","as_of":1770000000}'
```

## Production note

`HeaderPrincipalProvider` is for development / trusted networks. In production,
implement `PrincipalProvider` against your IdP — verify a JWT/OIDC token and
derive the tenant/policy/trust from its **verified** claims — so a client can
never assert a tenant it isn't entitled to. L5M enforces; your provider
authenticates.
