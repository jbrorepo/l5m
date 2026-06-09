# L5M Python client

A thin, **dependency-free** (stdlib `urllib` only) client for the
[L5M](https://github.com/jbrorepo/l5m) security-gated memory server. No
third-party supply chain to vet — a deliberate choice for a security product.

## Install

```bash
pip install l5m            # from PyPI (when published)
# or, from a checkout:
pip install ./clients/python
```

## Usage

```python
from l5m import Client, RateLimited, AuthError

# Header auth (dev / trusted network):
c = Client("http://localhost:8080", api_key="secret", tenant_id=7)

c.insert({
    "capsule_id": "1",
    "claim": "the launch is in March",
    "evidence": "the launch is in March",
    "source_id": 1,
    "valid_from": 1, "observed_at": 1, "last_verified_at": 1,
    "context_mask": "0xffff", "policy_mask": "0xffff",
    "trust_level": 8, "classification": 1, "poison_risk": 0,
})

resp = c.query("when is the launch?", max_capsules=5)
for cap in resp["frame"]["capsules"]:
    print(round(cap["score"], 3), cap["claim"])

c.delete(1)
```

### JWT (production)

In production the principal — tenant, policy, trust — comes from **verified JWT
claims**, so a client can never assert a tenant it isn't entitled to. Pass a
bearer token instead of `tenant_id`:

```python
c = Client("https://l5m.internal", bearer_token=my_jwt)
resp = c.query("quarterly revenue")
```

### Error handling

| Exception     | When                                    |
|---------------|-----------------------------------------|
| `AuthError`   | 401 / 403 — missing or invalid creds    |
| `RateLimited` | 429 — per-tenant rate limit exceeded    |
| `L5mError`    | any other HTTP or transport failure     |

```python
try:
    c.query("...")
except RateLimited:
    ...  # back off and retry
except AuthError:
    ...  # refresh token
```

## Why identity is never in the body

L5M runs **all** authorization gates *before* relevance scoring. This client puts
the principal only in auth headers / the bearer token — never in the request
body — so the server always scores under an authenticated identity. Even a
`tenant_id` placed in a capsule body is overridden by the server to the
authenticated tenant on write.

## API

| Method                              | Endpoint                  |
|-------------------------------------|---------------------------|
| `insert(capsule)`                   | `POST /v1/memories`       |
| `insert_many(capsules)`             | N × `POST /v1/memories`   |
| `query(text, ...)`                  | `POST /v1/query`          |
| `delete(id)`                        | `DELETE /v1/memories/:id` |
| `verify_audit()`                    | `GET /v1/audit/verify`    |
| `metrics()`                         | `GET /metrics`            |
| `healthz()`                         | `GET /healthz`            |

## License

MIT OR Apache-2.0
