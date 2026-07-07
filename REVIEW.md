# Reviewer's Guide

A map for anyone auditing L5M — security researchers, systems engineers,
prospective users. It tells you where the load-bearing code is, what's already
been checked, and where to point skepticism. If you find a hole, that's the
point; open an issue.

## The one claim everything rests on

> Every capsule that reaches scoring — and every capsule in the returned frame
> and its relation expansion — passes ALL hard gates (tenant, context, policy,
> temporal, trust), across both the immutable base and the mutable delta.

Authorization is enforced by **constructing the authorized candidate set
before scoring** (pre-filtering), never by filtering ranked results after the
fact. The semantic/ANN stage only ever sees the post-gate set, so an
unauthorized vector is never even scored. Attack this claim first.

## Where to read, in order (~30 min)

1. **`crates/l5m-core/src/retrieve.rs`** — the retrieval hot path. The gate
   scan runs first (`hard_gates_pass` / tenant posting slice), then candidate
   generation, then scoring. This is the file the security claim lives or dies
   in.
2. **`crates/l5m-core/src/store.rs`** — `MemoryStore`: base segments + LSM
   delta (bounded active buffer → sealed runs → compaction), WAL durability,
   `query()` merge with newest-tier-wins dedup + tombstones. Verify the delta
   is gated identically to the base.
3. **`crates/l5m-core/src/segment.rs`** — `from_untrusted_bytes` is the parser
   for fully untrusted input (the designated fuzz target). Every length is
   bounds-checked before allocation; the self-authenticating BLAKE3 hash is
   verified on open.
4. **`crates/l5m-server/src/principal.rs`** — the authn boundary. The principal
   (tenant/policy/trust) is resolved from verified credentials, **never** from
   the request body. `lib.rs` `authorize()` adds scope + rate-limit checks.
5. **`crates/l5m-mcp/src/lib.rs`** — the MCP server binds the principal at
   process start; tool arguments cannot widen it.

## What's already been audited (so you don't have to redo it)

- **No `unwrap`/`panic` on any untrusted-input surface.** Every `unwrap` in
  `l5m-server` and `l5m-core::segment` is inside `#[cfg(test)]`. The single
  non-test `expect` (`index.rs:266`) converts a fixed 32-byte BLAKE3 hash slice
  to `[u8; 8]` — provably infallible. Grep and confirm:
  `grep -rnE '\.unwrap\(\)|panic!' crates/l5m-server/src crates/l5m-mcp/src`.
- **`#![forbid(unsafe_code)]`** on every crate except `l5m-core`, which uses
  `#![deny(unsafe_code)]` with exactly one `#[allow(unsafe_code)]` on the mmap
  in `segment.rs::open` (documented with a SAFETY comment). That's the only
  `unsafe` in the workspace.
- **A real DoS was found here and fixed.** The parser fuzzer caught a 159 GB
  unbounded allocation from an unvalidated length field; the fix validates byte
  ranges before `Vec::with_capacity`. Both fuzzers guard against regressions.

## How to attack it (the tests that would catch you)

```bash
# The machine-checked invariant: randomized multi-tenant corpora + probes,
# asserts no returned capsule violates any gate.
cargo test -p l5m-core --test gate_invariant_proptest

# Perfect-match bypass attempts (a byte-identical secret in another tenant):
cargo test -p l5m-core --test adversarial_gates

# Dense-embedding bypass (can a cosine-near vector smuggle past the gate?):
cargo test -p l5m-core --test embeddings

# Parser robustness (20k adversarial mutations, must never panic/OOM/OOB):
cargo test -p l5m-core --lib open_never_panics

# The gate holds over HTTP and MCP, and scoped keys / tenant binding hold:
cargo test -p l5m-server --test api
cargo test -p l5m-mcp

# Runnable proof that exits non-zero if any gate leaks (also a CI gate):
cargo run -p l5m-core --example leak_demo
```

If you want to *break* it: add a case to `gate_invariant_proptest.rs`, or feed
`Segment::from_untrusted_bytes` a crafted input. A failing test is the most
useful bug report.

## Reproduce every performance/accuracy claim

- **Accuracy vs a real vector DB** (significance-tested, n=450):
  `SCORE_ANALYSIS.md` — commands included. Native hybrid beats Chroma+MiniLM on
  R@5/@10/MRR at p ≤ 0.01.
- **The gate differentiator** (100K docs, 100 tenants, zero API keys):
  `reports/GATED_RETRIEVAL.md` — 0/26 embargoed disclosures vs 92% for a
  perfectly-filtered vector DB. `cargo build --release -p l5m-bench` then the
  two commands in that report.
- **Latency/scale microbenchmarks:** `OPTIMIZATION_REPORT.md`.

## Known limitations (stated so you don't have to find them)

- **Single-writer.** One process owns the data dir; HA/read-replicas are a
  designed-not-built roadmap item (`deploy/HA.md`). Do not read "distributed".
- **Deterministic fingerprints lose to learned embeddings on paraphrase.**
  On the synthetic gated benchmark, Chroma out-ranks L5M on clean-needle recall
  (0.82 vs 0.51) — reported, not hidden. Real-text hybrid mode closes this
  (`SCORE_ANALYSIS.md`); the fingerprint alone is weak on paraphrase.
- **Encryption in transit is the deployer's job** (terminate TLS in front).
  Gates are correctness-verified, not constant-time; timing side-channels are
  out of scope. See `THREAT_MODEL.md`.
- **Benchmark corpora are single-tenant** (LongMemEval/ConvoMem), which is why
  the gate benchmark is synthetic — the public datasets don't exercise gates.

## Threat model & security policy

`THREAT_MODEL.md` (assets → trust boundaries → threats → mitigations → tests)
and `SECURITY.md` (disclosure, CI assurance, cosign verification). Compliance
control mapping with executable evidence: `COMPLIANCE.md`.
