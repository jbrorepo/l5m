# L5M Threat Model

L5M is a memory-retrieval engine for LLM/agent systems. Its defining property is
**gate-before-scoring**: authorization, policy, trust, and temporal checks run on
the candidate set *before* any relevance scoring, so an unauthorized memory can
never surface — not even by being the most semantically or lexically similar to
the query. This document states what L5M defends, what it does not, and how each
guarantee is verified.

## Assets

- **Memory capsules** — claims/evidence plus metadata (tenant, context, policy
  mask, trust level, validity window, source/content hashes, embedding).
- **Tenant isolation** — capsules of one tenant must never reach another.
- **Provenance/integrity** — returned evidence must be attributable and
  tamper-evident (source + content BLAKE3 hashes; self-authenticating segment).

## Trust boundaries

1. **Caller → engine.** The caller supplies a `MemoryProbe` (tenant id, context
   mask, caller policy mask, trust floor, as-of time, optional query embedding).
   The engine trusts the caller to present the *correct principal context*; it is
   the caller's job (its authn/z layer) to populate the probe truthfully. Given a
   probe, the engine enforces every gate exactly.
2. **Segment file → engine.** Compiled `.segment` files are **untrusted input**.
   L5M memory-maps them and treats every byte as adversarial.
3. **Memory content → LLM.** Capsule text is **data, never instructions**. L5M
   returns it verbatim with provenance; it never executes or interprets it.

## Attacker model & in-scope threats

| Threat | Mechanism | Mitigation | Verified by |
|---|---|---|---|
| **Cross-tenant data access** | Query as tenant A, hope to read tenant B | Tenant gate; queries scan only the tenant's own postings | `adversarial_gates::tenant_*`, `multi_tenant_scan_is_isolated_and_complete`, proptest |
| **Authorization bypass via similarity** | Craft a query that is a perfect lexical/dense match to a restricted capsule | Gates run **before** scoring/ANN/fusion; only authorized capsules are ever scored | `adversarial_gates::*`, `embeddings::dense_match_cannot_bypass_tenant_gate`, `candidate_cap_does_not_bypass_gates` |
| **Clearance/least-privilege violation** | Read above one's policy/clearance | Policy-mask gate (bitwise-overlap), trust-floor gate | proptest invariant, `policy_gate_*`, `trust_gate_*` |
| **Stale/revoked data exposure** | Read expired or not-yet-valid memory | Temporal gate (valid_from ≤ as_of ≤ valid_until) + supersession | `temporal_gate_*`, `superseded_old_policy_*` |
| **Prompt injection via stored memory** | Poison a memory with "ignore instructions…" | Content is data-only; trust floor + poison-risk metadata keep low-trust content out | `prompt_injection_like_memory_is_excluded_at_high_trust` |
| **Malicious segment file (DoS / memory corruption)** | Hand-crafted `.segment` bytes | BLAKE3 self-hash, monotonic section validation, bounds checks on every field, **untrusted lengths validated before allocation**; pure-safe Rust except one audited mmap | `open_never_panics_on_adversarial_bytes` (20k-mutation fuzzer), `loader_rejects_*` |
| **Tamper / silent corruption** | Flip bytes in evidence or metadata | Per-capsule content & source hashes + whole-segment hash | `loader_rejects_content_hash_mismatch_*` |
| **Result exfiltration of unauthorized neighbors via relation expansion** | Pull restricted capsules through "supports/contradicts" edges | Relation expansion re-applies gates; a documented, request-controlled metadata exception only for contradictions | `relation_expansion_does_not_leak_unauthorized_related_capsules` |

## Out of scope (caller's responsibility)

- **Authentication / principal resolution.** L5M enforces the probe it is given;
  it does not authenticate the caller or decide what tenant/policy they hold.
- **Encryption at rest / in transit.** Segments are integrity-protected
  (hashes), not encrypted. Use OS/disk encryption and transport security.
- **Side channels.** Timing/cache side channels are not addressed; gate
  *correctness* is, not constant-time execution.
- **Embedding model trust.** Embeddings are computed offline by the caller; L5M
  treats vectors as opaque inputs.
- **Availability under resource exhaustion** beyond parser DoS (e.g. disk
  pressure) is an operational concern.

## Security properties & how they're proven

1. **Gate-before-scoring (no authorization bypass).** Enforced in
   `retrieve.rs`: the candidate set is built from gate predicates first; scoring,
   ANN, and dense fusion only ever see authorized capsules. Proven by a
   property-based test over randomized multi-tenant corpora/probes
   (`gate_invariant_proptest`, 400 cases) plus targeted perfect-match adversarial
   tests.
2. **Tenant isolation is structural.** A probe iterates only its tenant's
   ordinals; cross-tenant capsules are never visited.
3. **Hardened untrusted-input parser.** `Segment::open` is continuously fuzzed;
   it returns a typed error (never panics, OOMs, or reads out of bounds) on
   adversarial bytes. Untrusted length fields are bounds-validated before any
   allocation (a real unbounded-allocation DoS was found by the fuzzer and fixed).
4. **Memory safety.** `#![forbid(unsafe_code)]` across the workspace except one
   audited, fuzzed `unsafe` (the read-only mmap), which is `#![deny]`-gated and
   documented.
5. **Provenance & tamper-evidence.** Every result carries source/content hashes;
   the segment authenticates itself with a BLAKE3 digest.

## Residual risks / honest limitations

- Gate **correctness** is verified; **constant-time** execution is not.
- Segments are **not encrypted**; integrity ≠ confidentiality at rest.
- The metadata exception in contradiction expansion is intentional and
  request-gated — review it for your policy before enabling contradictions.
- Embedding quality/poisoning is delegated to the caller's embedding pipeline.

Report suspected vulnerabilities per [SECURITY.md](SECURITY.md).
