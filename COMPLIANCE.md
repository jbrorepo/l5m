# Compliance mapping — product controls with executable evidence

This document maps L5M's **product-level controls** to the frameworks
enterprise security reviews evaluate: SOC 2 Trust Services Criteria,
ISO/IEC 27001:2022 Annex A, GDPR, and the HIPAA Security Rule.

**Honest framing, up front:** L5M (the software) is not "SOC 2 compliant" —
no software is. SOC 2 / ISO certifications attest to an *organization's*
controls. What this document gives you is the half a vendor usually can't:
for every product control we claim, a **runnable test or CI job that proves
it**. Use it to accelerate vendor security questionnaires, DDQs, and your own
audit scoping. Organizational controls remain yours (or ours, in a managed
offering) — the last section lists exactly which.

A useful structural fact for your review: in a self-hosted deployment, memory
content **never leaves your boundary**. L5M is a single binary with zero
runtime phone-home, zero external service dependencies, and no model
inference on the query path.

---

## 1. Access control & multi-tenant isolation

The core architectural control: authorization gates (tenant, context/
clearance, trust, temporal) execute **before** relevance scoring. An
unauthorized memory is never a retrieval candidate, so isolation does not
depend on application-layer filtering discipline.

| Framework | Criteria |
|---|---|
| SOC 2 | CC6.1 (logical access), CC6.3 (role-based restriction) |
| ISO 27001 | A.5.15 (access control), A.8.3 (information access restriction), A.5.10 (acceptable use of information) |
| GDPR | Art. 25 (data protection by design and by default), Art. 32(1)(b) (confidentiality) |
| HIPAA | §164.312(a)(1) (access control), §164.308(a)(4) (information access management) |

Product mechanisms: per-tenant store-layer isolation; context/policy bitmask
gates; trust floors; scoped API keys (`read`/`write`/`admin`) with optional
tenant binding that request headers cannot override; JWT/OIDC principals
derived from verified claims only; MCP principal bound at process start
(prompt-injected agents cannot widen scope).

**Executable evidence**
```bash
cargo test -p l5m-core --test adversarial_gates        # perfect-match bypass attempts fail
cargo test -p l5m-core --test gate_invariant_proptest  # machine-checked invariant, randomized corpora
cargo run  -p l5m-core --example leak_demo             # exits non-zero on any leak (runs in CI)
cargo test -p l5m-server --test api scoped_keys        # read key 403s on write; tenant binding wins
cargo test -p l5m-mcp                                  # hostile tool args cannot override principal
```

## 2. Encryption at rest

| Framework | Criteria |
|---|---|
| SOC 2 | CC6.1, CC6.7 (transmission/storage protection) |
| ISO 27001 | A.8.24 (use of cryptography) |
| GDPR | Art. 32(1)(a) (encryption of personal data) |
| HIPAA | §164.312(a)(2)(iv) (encryption and decryption) |

Product mechanisms: sealed segments via ChaCha20-Poly1305 AEAD
(`encryption` feature); `compile_segment_sealed` never writes plaintext to
disk; pluggable `KeyProvider` (static/env today, KMS-ready interface); AEAD
tag rejects wrong-key and tampered ciphertext. *Encryption in transit is a
deployment concern (terminate TLS in front of the server) — see §8.*

**Executable evidence**
```bash
cargo test -p l5m-core --features encryption --test encryption
```

## 3. Audit logging & accountability

| Framework | Criteria |
|---|---|
| SOC 2 | CC7.2 (monitoring for anomalies), CC4.1 (evidence of operation) |
| ISO 27001 | A.8.15 (logging), A.5.28 (collection of evidence) |
| GDPR | Art. 30 (records of processing), Art. 5(2) (accountability) |
| HIPAA | §164.312(b) (audit controls) |

Product mechanisms: hash-chained, tamper-evident audit log recording every
disclosure — principal context, query hash, gate/candidate counts, disclosed
capsule ids + source hashes. Any edit, deletion, or reorder breaks the chain;
`GET /v1/audit/verify` re-verifies it on demand. Answers the forensic
question: *what did the AI disclose, to whom, and was the record altered?*

**Executable evidence**
```bash
cargo test -p l5m-core --test audit
cargo test -p l5m-server --test api queries_are_audited_and_chain_verifies
```

## 4. Resilience & recoverability

| Framework | Criteria |
|---|---|
| SOC 2 | CC7.5 (recovery), A1.2 (availability commitments) |
| ISO 27001 | A.8.13 (information backup), A.8.14 (redundancy) |
| HIPAA | §164.308(a)(7) (contingency plan), §164.312(c) (integrity) |

Product mechanisms: write-ahead log fsync'd before any write is acknowledged
(acknowledged writes survive crashes); O(N) replay on restart; crash-safe
checkpointing (`compact_to` writes the checkpoint *before* truncating the
WAL, so a crash mid-compaction loses nothing); self-authenticating segments
(BLAKE3 whole-file + per-capsule hashes) detect storage corruption on open.

**Executable evidence**
```bash
cargo test -p l5m-core --test durability   # restart survival, crash-window consistency
```

## 5. Abuse resistance & availability

| Framework | Criteria |
|---|---|
| SOC 2 | CC6.6 (boundary protection), A1.1 (capacity) |
| ISO 27001 | A.8.6 (capacity management), A.8.7 (protection against malware/abuse) |
| GDPR | Art. 32(1)(b) (resilience of processing systems) |

Product mechanisms: per-tenant token-bucket rate limiting (one noisy tenant
cannot starve others; over budget → 429); request body size caps; metrics
cardinality caps (tenant-id spraying cannot exhaust memory); untrusted-input
parser hardened against allocation attacks — coverage-guided fuzzing found
and fixed a real unbounded-allocation DoS before release, and both fuzzers
run continuously in CI as regression guards.

**Executable evidence**
```bash
cargo test -p l5m-server --test api rate_limiter_returns_429
cargo test -p l5m-core --lib open_never_panics          # 20k adversarial mutations
cargo +nightly fuzz run segment_parse -- -max_total_time=60   # libFuzzer (also in CI)
```

## 6. Data lifecycle: erasure, retention, point-in-time

| Framework | Criteria |
|---|---|
| GDPR | Art. 17 (right to erasure), Art. 5(1)(e) (storage limitation), Art. 16 (rectification) |
| SOC 2 | CC6.5 (disposal) |
| ISO 27001 | A.8.10 (information deletion) |
| HIPAA | §164.310(d)(2)(i) (disposal) |

Product mechanisms and their precise semantics — stated exactly, because
auditors will ask:

- **`DELETE /v1/memories/{id}`** tombstones a memory: it is immediately and
  permanently excluded from every query result.
- **Physical erasure** = delete **+ compaction**. Tombstoned bytes may remain
  inside immutable segment files until `compact`/`compact_to` rewrites the
  base. For an Art. 17 erasure workflow: delete the ids, then run a
  compaction; the rewritten segment contains no trace of the erased records.
- **Rectification** (Art. 16): re-inserting an id replaces the prior version
  (newest-wins, proven across all storage tiers).
- **Retention**: `valid_until` expires a memory from results automatically
  (temporal gate); storage-limitation policies map onto it directly.
- **Point-in-time accountability**: bi-temporal capsules + `as_of` queries
  reconstruct *what the system would have disclosed at time T* — useful for
  incident reconstruction and DSAR responses.

**Executable evidence**
```bash
cargo test -p l5m-core --test mutable_store    # tombstones, newest-wins, compaction drops deleted data
cargo test -p l5m-mcp point_in_time            # as_of time-travel recall
```

## 7. Secure development & supply chain

| Framework | Criteria |
|---|---|
| SOC 2 | CC8.1 (change management) |
| ISO 27001 | A.8.25–A.8.31 (secure SDLC), A.5.21 (supply chain), A.8.28 (secure coding) |

Product mechanisms: memory-safe Rust with `forbid(unsafe_code)` everywhere
except one documented, fuzzed mmap; `cargo-deny` in CI (advisories, license
allowlist, source bans); minimal dependency tree by policy (the SDKs have
**zero** runtime dependencies); every release ships cross-platform binaries
with SHA-256 checksums, CycloneDX **and** SPDX SBOMs, and keyless cosign
(Sigstore) signatures verifiable against GitHub OIDC identity; 3-OS test
matrix + clippy `-D warnings` + security suite on every push.

**Executable evidence**
```bash
cargo deny check advisories licenses bans sources
cosign verify-blob --certificate <f>.cert --signature <f>.sig \
  --certificate-identity-regexp 'https://github.com/jbrorepo/l5m/.*' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com <f>
```

## 8. Organizational controls — explicitly out of product scope

A complete compliance program needs controls no library/server can provide.
In a self-hosted deployment these are the operator's responsibility; in a
managed offering they would be covered by the provider's SOC 2 program:

- **TLS termination / encryption in transit** (reverse proxy or service mesh)
- **Identity provider** (the JWT/JWKS hooks consume your IdP; they don't replace it)
- **Infrastructure security** (host hardening, network policy, secrets management)
- **Backup execution & DR drills** (the WAL/checkpoint primitives support them; running them is operational)
- **Personnel, vendor management, incident response process, risk assessments**
- **Certification itself** (SOC 2 Type II / ISO 27001 audits attest organizations)

## Verifying this entire document in one command

Every product claim above is exercised by CI on every push:

```bash
cargo test --workspace && cargo run -p l5m-core --example leak_demo
```

Questions, or need a filled security questionnaire (CAIQ/SIG)? Open an issue
or see [SECURITY.md](SECURITY.md) for contact and disclosure policy.
