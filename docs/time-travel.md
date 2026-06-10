# Time-travel recall: bi-temporal memory

Most memory systems store *what is believed now*. L5M stores *what was true
when, and what was believed when* — and lets you query either. This is a
first-class capability of the engine (the temporal gate runs before scoring,
like every other gate), not a bolt-on filter.

## The bi-temporal model

Every memory capsule carries two independent time axes:

| Axis | Fields | Question it answers |
|---|---|---|
| **Validity** (real-world time) | `valid_from`, `valid_until` | *When was this fact true?* |
| **Knowledge** (system time) | `observed_at`, `last_verified_at` | *When did we learn / last confirm it?* |

```jsonc
{
  "claim": "the office is at 12 Amber Street",
  "valid_from": 1700000000,        // fact became true
  "valid_until": 1731536000,       // fact stopped being true (omit = still true)
  "observed_at": 1700090000,       // when the system learned it
  "last_verified_at": 1722222222   // when it was last confirmed
}
```

## Point-in-time queries: `as_of`

Every query surface accepts `as_of` (unix seconds). The temporal gate then
admits only capsules whose validity window contains that instant —
**before scoring**, so an expired or not-yet-valid memory is never even a
candidate, never ranked, never leaked into context.

```bash
# HTTP
curl -s localhost:8080/v1/query -X POST -H 'content-type: application/json' \
  -H 'x-l5m-api-key: secret' -H 'x-l5m-tenant: 1' \
  -d '{"query": "where is the office?", "as_of": 1715000000}'
```

```python
# Python SDK
c.query("where is the office?", as_of=1715000000)
```

```ts
// TypeScript SDK
await c.query("where is the office?", { asOf: 1715000000 });
```

```
MCP (agent tool call): recall { "query": "where is the office?", "as_of": 1715000000 }
```

Omit `as_of` to query the present (all currently-valid memories).

## Facts that change: supersession without amnesia

When a fact changes, you don't overwrite history — you close the old validity
window and insert the new fact:

```jsonc
// 1. Close the old fact (re-insert id 17 with an end date)
{ "capsule_id": "17", "claim": "office at 12 Amber Street",
  "valid_from": 1700000000, "valid_until": 1731536000, ... }

// 2. Insert the new fact
{ "capsule_id": "18", "claim": "office at 3 Cobalt Avenue",
  "valid_from": 1731536000, ... }
```

Now the same question gives time-correct answers:

| Query | Answer |
|---|---|
| `as_of` = May 2024 | 12 Amber Street |
| `as_of` omitted (now) | 3 Cobalt Avenue |

Vector stores overwrite or duplicate; L5M keeps both with their windows, and
the temporal gate picks the right one structurally. (Re-inserting the *same*
id replaces the prior version — newest-wins — when you do want rectification
rather than history; see [COMPLIANCE.md](../COMPLIANCE.md) §6.)

## Retention for free

`valid_until` doubles as a TTL: set it at write time and the memory ages out
of all results automatically when its window closes — no cron, no cleanup
job, no "forgot to filter" risk, because expiry is enforced by the same gate
that enforces tenancy. Physical removal happens at the next compaction.

## What this is for

- **"What did we believe last quarter?"** — agent and analyst queries against
  the historical knowledge state, not just the current one.
- **Incident reconstruction / forensics** — combined with the hash-chained
  audit log: *what would the system have disclosed at time T, and what did it
  actually disclose?*
- **DSAR / compliance responses** — reconstruct the knowledge state at a
  regulator-specified date.
- **Contracts, pricing, policies** — domains where the valid-at date is the
  whole question.

## Proof

The temporal gate is covered by the same machinery as every other gate:

```bash
cargo test -p l5m-core --test gate_invariant_proptest   # temporal invariant, randomized
cargo test -p l5m-mcp point_in_time                     # as_of over MCP
cargo test -p l5m-core --test retrieval                 # validity-window gating
```
