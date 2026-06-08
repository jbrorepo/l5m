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

**Out of scope (caller's responsibility):** authentication/principal resolution,
encryption at rest/in transit, side-channel resistance, and the trustworthiness
of the embedding model. See the threat model for details.

## Hardening & assurance in CI

Every change runs: `cargo test` (incl. the gate-invariant property test and the
segment-parser fuzzer), `cargo clippy -D warnings`, `cargo fmt --check`,
`cargo deny` (advisories, licenses, supply-chain bans). Releases are intended to
ship with signed artifacts and an SBOM.

## Verifying the security claims yourself

```bash
cargo test -p l5m-core --test gate_invariant_proptest    # gate invariant (randomized)
cargo test -p l5m-core --test adversarial_gates          # perfect-match bypass attempts
cargo test -p l5m-core --test embeddings                 # dense match cannot bypass gates
cargo test -p l5m-core --lib open_never_panics           # parser fuzzer (20k mutations)
```
