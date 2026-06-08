# Changelog

All notable changes to L5M are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions follow SemVer once 1.0.

## [Unreleased]

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
