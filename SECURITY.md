# Security Policy

## Reporting a vulnerability

**Please do not open public issues for security problems.** Report privately via
GitHub Security Advisories ("Report a vulnerability" on the repo's Security tab),
or email the maintainers listed in `Cargo.toml`.

- We acknowledge reports within **3 business days**.
- We aim to provide a remediation plan within **10 business days**.
- We credit reporters in the advisory and `CHANGELOG.md` unless you prefer to
  remain anonymous.

Please include: affected version/commit, a minimal reproduction (a crafted
`.segment` file or probe is ideal), and the impact you observed.

## Supported versions

Until 1.0, only the latest `main` and the most recent tagged release receive
security fixes.

## What L5M defends (and what it does not)

The full analysis is in [THREAT_MODEL.md](THREAT_MODEL.md). In brief:

**Defended, with tests as evidence:**
- **Authorization is enforced before scoring** — no query (lexical or dense) can
  surface a capsule that fails the tenant/context/policy/trust/temporal gates.
  Proven by a property-based invariant test over randomized corpora and by
  perfect-match adversarial tests.
- **Untrusted segment files** are parsed defensively: self-authenticating BLAKE3
  hash, full bounds checking, untrusted lengths validated before allocation. The
  loader is continuously fuzzed and must never panic, over-allocate, or read out
  of bounds.
- **Memory safety:** `#![forbid(unsafe_code)]` workspace-wide except one audited,
  documented, fuzzed mmap.
- **Provenance & tamper-evidence:** per-result source/content hashes.

**Available enterprise controls:**
- **Encryption at rest** (`encryption` feature) — AEAD-sealed segments with a
  customer-supplied key (KMS-ready `KeyProvider`); plaintext never hits disk.
- **Tamper-evident audit log** — hash-chained record of every disclosure
  (principal context, query hash, what was returned + source hashes), with
  `verify_audit_chain` to detect any forgery.

**Out of scope (caller's responsibility):** authentication/principal resolution,
encryption *in transit*, side-channel resistance, and the trustworthiness of the
embedding model. See the threat model for details.

## Hardening & assurance in CI

Every change runs: `cargo test` (incl. the gate-invariant property test and the
segment-parser fuzzer), the gate-before-scoring leak demo, the Python SDK tests,
a bounded `cargo-fuzz` campaign, `cargo clippy -D warnings`, `cargo fmt --check`,
and `cargo deny` (advisories, licenses, supply-chain bans).

**Tagged releases** (`.github/workflows/release.yml`) publish cross-platform
binaries plus a CycloneDX **and** SPDX SBOM, each with a SHA-256 checksum and a
**keyless cosign (Sigstore) signature**. Verify any artifact with:

```bash
cosign verify-blob \
  --certificate <file>.cert --signature <file>.sig \
  --certificate-identity-regexp 'https://github.com/jbrorepo/l5m/.*' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  <file>
```

## Verifying the security claims yourself

```bash
cargo test -p l5m-core --test gate_invariant_proptest    # gate invariant (randomized)
cargo test -p l5m-core --test adversarial_gates          # perfect-match bypass attempts
cargo test -p l5m-core --test embeddings                 # dense match cannot bypass gates
cargo test -p l5m-core --lib open_never_panics           # parser fuzzer (20k mutations)
cargo run  -p l5m-core --example leak_demo               # gate-before-scoring proof (exits non-zero on leak)
```

### Coverage-guided fuzzing (cargo-fuzz)

In addition to the in-tree mutational fuzzer above, the segment parser has a
libFuzzer target driven by [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz)
(`fuzz/fuzz_targets/segment_parse.rs`), run in CI for a bounded session and
runnable locally for longer campaigns:

```bash
cargo install cargo-fuzz
cargo +nightly fuzz run segment_parse                       # until a crash / Ctrl-C
cargo +nightly fuzz run segment_parse -- -max_total_time=60 # bounded
```

The invariant: `Segment::from_untrusted_bytes` must reject arbitrary input with
an `Err` — never panic, over-allocate, or read out of bounds. (Fuzzing already
caught and fixed a real 159 GB-allocation DoS from an unvalidated length field.)
