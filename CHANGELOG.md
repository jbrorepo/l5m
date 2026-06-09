# Changelog

All notable changes to L5M are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions follow SemVer once 1.0.

## [Unreleased]

### Enterprise
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
