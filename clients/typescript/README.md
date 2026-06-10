# L5M TypeScript client

A thin, **zero-runtime-dependency** TypeScript/JavaScript client for the
[L5M](https://github.com/jbrorepo/l5m) security-gated memory server. Built on
the standard `fetch` — works in Node 18+, browsers, and edge runtimes. No
third-party supply chain to vet.

## Install

```bash
npm install l5m-client          # from npm (when published)
# or, from a checkout:
npm install ./clients/typescript
```

## Usage

```ts
import { Client, AuthError, RateLimited } from "l5m-client";

// Header auth (dev / trusted network):
const c = new Client("http://localhost:8080", { apiKey: "secret", tenantId: 7 });

await c.insert({
  capsule_id: "1",
  claim: "the launch is in March",
  evidence: "the launch is in March",
  source_id: 1,
  valid_from: 1, observed_at: 1, last_verified_at: 1,
  context_mask: "0xffff", policy_mask: "0xffff",
  trust_level: 8, classification: 1, poison_risk: 0,
});

const res = await c.query("when is the launch?", { maxCapsules: 5 });
for (const cap of res.frame.capsules) {
  console.log(cap.score.toFixed(3), cap.claim);
}

// Point-in-time recall — what did we believe at t=1700000000?
const then = await c.query("office location", { asOf: 1_700_000_000 });

await c.delete("1");
```

### JWT (production)

In production the principal — tenant, policy, trust — comes from **verified JWT
claims**, so a client can never assert a tenant it isn't entitled to:

```ts
const prod = new Client("https://l5m.internal", { bearerToken: myJwt });
```

### Metering (admin scope)

```ts
const rows = await admin.usage(); // [{ tenant: 7, queries: 120, inserts: 14, ... }]
```

### Error handling

| Class         | When                                                  |
|---------------|-------------------------------------------------------|
| `AuthError`   | 401 / 403 — bad credentials or insufficient key scope |
| `RateLimited` | 429 — per-tenant rate limit exceeded                  |
| `L5mError`    | any other HTTP or transport failure                   |

## Why identity is never in the body

L5M runs **all** authorization gates *before* relevance scoring. This client
puts the principal only in auth headers / the bearer token — never in the
request body — so the server always scores under an authenticated identity.
Even a `tenant_id` in a capsule body is overridden by the server on write.

## Development

```bash
npm install
npm test     # tsc + node:test against an in-process stub server
```
