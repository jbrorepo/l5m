# Changelog

All notable changes to L5M are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions follow SemVer once 1.0.

## [Unreleased]

### Access control
- **Scoped API keys**: `L5M_API_KEYS=secret:scope[:tenant],…` configures
  multiple keys, each `read` | `write` | `admin`. Queries require `read`;
  inserts/deletes require `write`; over-scope requests get HTTP 403. A key may
  be **tenant-bound**, in which case it can only ever act as that tenant —
  the binding overrides whatever `X-L5M-Tenant` claims (tested).
- **JWT `scope` claim**: bearer tokens may carry `scope: read|write|admin`
  (default write), enforced identically.
- **JWKS key rotation** (`L5M_JWT_JWKS_FILE`): RS256 verification keys loaded
  from a standard JWKS document with `kid`-based selection, hot-reloaded when
  the file changes — rotate IdP keys with zero server restarts. A malformed
  mid-rotation file keeps the previous keys (fail-safe). No HTTP client added
  to the binary; fetching the JWKS is a sidecar/cron concern. Tests cover
  kid selection, forged-key rejection, and live rotation (old key retired,
  new key accepted, same process).

### Operations
- **Admin/ops API** (all endpoints require an admin-scope credential):
  `GET /v1/admin/stats` (segments, active buffer, sealed runs, tombstones,
  durability); `POST /v1/admin/compact` (minor compaction on demand);
  `POST /v1/admin/checkpoint` (durable fold into a timestamped base segment
  under server-configured `L5M_CHECKPOINT_DIR` + crash-safe WAL truncation —
  the path is never client-supplied, so an admin credential cannot be turned
  into a path-traversal write); `GET /v1/admin/audit/export` (raw hash-chained
  JSONL for SIEM ingestion). `MemoryStore::stats()` added in core. Tests cover
  the full ops cycle (write → stats → compact → checkpoint-to-disk → query
  still serves) plus admin-only guards on every endpoint and audit export
  content.

### Metering & observability
- **Per-tenant usage metrics**: queries, capsules returned, inserts, and
  deletes are now attributed to the tenant that incurred them — the metering
  record for billing/quotas. Exposed two ways: Prometheus series with a
  `tenant` label (`l5m_tenant_queries_total{tenant="7"}` …) and a JSON
  `GET /v1/usage` endpoint requiring an **admin-scope** credential.
  Cardinality is capped (10k tenants; overflow aggregates into `other`) so a
  tenant-id-spraying client cannot blow up metrics memory.

### Integrations
- **TypeScript SDK** (`clients/typescript`): zero-runtime-dependency client on
  the standard `fetch` (Node 18+, browsers, edge). Typed surface — `Capsule`,
  `QueryResponse`, `UsageRow` — with `insert` / `query` (incl. `asOf`
  time-travel) / `delete` / `usage` / `verifyAudit` / `metrics` / `healthz`,
  and typed errors (`AuthError` 401/403, `RateLimited` 429). 11 tests against
  an in-process `node:http` stub via `node:test`; CI job added. Python SDK
  gains `usage()` for parity.
- **MCP server** (`l5m-mcp` crate): security-gated memory for any MCP host
  (Claude Desktop/Code, ChatGPT, Copilot, custom agents) over stdio JSON-RPC.
  Tools: `remember` / `recall` (with point-in-time `as_of`) / `forget`. The
  principal (tenant/context/policy/trust floor) is bound **once at process
  start** from host configuration — tool arguments cannot name a tenant or
  widen a mask, so even a prompt-injected agent stays inside its granted memory
  slice. Durable by default (WAL-backed `L5M_DATA_DIR`). Dependencies:
  `l5m-core` + `serde_json` only (no MCP SDK, no async runtime). Protocol +
  security tests, including hostile-argument and cross-tenant recall attempts.

### Enterprise
- **JWT/OIDC authentication** for the server: a `JwtPrincipalProvider` derives
  the principal from a cryptographically verified `Authorization: Bearer` token
  (HS256 shared secret or RS256/OIDC public key), with signature + expiry checks.
  Configure via `L5M_JWT_HS256_SECRET` / `L5M_JWT_RS256_PEM_FILE`.
- **Audit wired into the server**: when `L5M_AUDIT_LOG` is set, every query emits
  a tamper-evident record, and `GET /v1/audit/verify` confirms chain integrity.
- **Durable real-time writes**: a write-ahead log (`wal.rs`) fsync'd per
  mutation so acknowledged inserts/deletes survive a crash/restart;
  `MemoryStore::open_durable` replays it; `compact_to` checkpoints the live state
  to a base segment and truncates the WAL in a crash-safe order.
- **Single-tenant dense ANN**: a gate-filtered cosine-LSH (random-hyperplane
  SimHash) over stored embeddings surfaces cosine-near memories even when they
  share no lexical signal — closing dense recall *within* a large tenant —
  without ever considering an unauthorized capsule.
- **HTTP server** (`l5m-server` crate): deployable service exposing gated
  retrieval, real-time insert/delete, `/healthz`, `/readyz`, and `/metrics`. The
  principal (tenant/policy/trust) is resolved from the request by a pluggable
  `PrincipalProvider` (never the body), so the gates run under an authenticated
  identity; writes are forced to the caller's tenant. Structured JSON logs,
  graceful shutdown, Dockerfile. The full gate-before-scoring + tenant-isolation
  guarantee is enforced over the network (tested).
- **Observability**: dependency-free Prometheus metrics (queries, returns,
  candidates scored, inserts/deletes, latency histogram) wired into the store and
  exposed at `/metrics`.
- **Encryption at rest** (`encryption` feature): sealed segments via ChaCha20-
  Poly1305 AEAD with a `KeyProvider` abstraction (static/env, KMS-ready).
  `compile_segment_sealed` never writes plaintext to disk; `Segment::open_sealed`
  decrypts in-memory. Wrong key / tampering are rejected by the AEAD tag.
- **Tamper-evident audit log**: every query can emit a hash-chained `AuditRecord`
  (principal context, query hash, gate/candidate counts, disclosed capsule ids +
  source hashes). `verify_audit_chain` detects any edit, deletion, or reorder —
  forensic/compliance evidence for "what did the AI disclose, to whom, and was it
  allowed?".

### Security
- **Hardened the untrusted-input segment parser.** Added a 20,000-mutation
  fuzz test (`open_never_panics_on_adversarial_bytes`) that proves `Segment::open`
  never panics, over-allocates, or reads out of bounds on malformed input.
- **Fixed an unbounded-allocation DoS** found by the fuzzer: untrusted relation
  and string-list length fields were used as allocation capacities before being
  bounds-checked. Lengths are now validated against available bytes first.
- **Machine-checked the core security invariant** with a property-based test
  (`gate_invariant_proptest`, 400 randomized multi-tenant cases): no probe can
  surface a capsule that fails the tenant/context/policy/trust/temporal gates.
- **`#![forbid(unsafe_code)]`** across the workspace except one audited,
  documented, fuzzed `unsafe` (the read-only mmap), gated with `#![deny]`.
- Added [`THREAT_MODEL.md`](THREAT_MODEL.md), [`SECURITY.md`](SECURITY.md),
  `cargo-deny` supply-chain policy, and a CI workflow running tests, clippy,
  fmt, the security suite, and supply-chain checks.

### Added
- Native **hybrid (lexical ⊕ dense) retrieval**: dense embeddings stored in the
  segment + query embedding on the probe + RRF fusion inside `retrieve`.
  Statistically more accurate than a real vector DB on held-out (R@5/R@10/MRR,
  p ≤ 0.01).
- **Real-time mutable layer**: `MemoryStore` insert/update/delete (tombstones) +
  compaction over an in-memory delta segment.
- **Sublinear retrieval at scale**: tenant-scoped gates (0.88 ms @ 1M / 1000
  tenants) and an LSH semantic index (proven top-1 identical to exact scan).
- Honest benchmark harness with build-inclusive latency + a real vector-DB peer
  (Chroma + all-MiniLM-L6-v2); `fuse-runs` and `embed-run` subcommands.

### Changed
- **Corrected previously inflated benchmark claims** in the README. All claims
  are now independently re-derived and significance-tested; see
  [`VALIDATION_REPORT.md`](VALIDATION_REPORT.md) and [`SCORE_ANALYSIS.md`](SCORE_ANALYSIS.md).
