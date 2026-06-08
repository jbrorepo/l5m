# Contributing to L5M

Thanks for your interest. L5M is security-critical infrastructure, so the bar
for changes is correctness and honesty.

## Ground rules

1. **Never weaken a gate.** Any change touching `retrieve.rs`, `segment.rs`, or
   the gate logic must keep the security tests green:
   ```bash
   cargo test -p l5m-core --test gate_invariant_proptest --test adversarial_gates --test embeddings
   cargo test -p l5m-core --lib open_never_panics_on_adversarial_bytes
   ```
2. **Benchmarks must be honest.** No claim ships without a reproducible command
   and, for accuracy claims, a significance test (`scripts/score_analysis.py`).
   Build-inclusive latency where the build is per-query.
3. **Memory safety.** The workspace is `forbid(unsafe_code)` except the one
   audited mmap. New `unsafe` will not be accepted without a strong, fuzzed
   justification.
4. **Untrusted input.** Anything parsing a segment file must bounds-check before
   indexing and validate lengths before allocating. Add a fuzz case for new
   parsing paths.

## Before opening a PR

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --release
cargo deny check        # if cargo-deny is installed
```

## Reporting security issues

Do **not** open a public issue. See [SECURITY.md](SECURITY.md).
