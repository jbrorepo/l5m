# Admissible Memory

Most agent memory systems ask: "What past text is similar to this question?"

L5M asks a stricter question: "What memory is relevant, allowed, current, trusted, and provable enough to show the model?"

## Plain English Flow

L5M turns memory into cards. Each card carries:

- the claim
- the evidence
- the tenant
- the valid time window
- context and policy masks
- trust level
- source hash
- relation/conflict metadata

At query time, L5M checks the gates before ranking:

1. Is it the right tenant?
2. Is it allowed in this context?
3. Does policy allow the caller to see it?
4. Is it valid at the requested time?
5. Is it trusted enough?
6. Only then: how well does it match the question?

The result is a `MemoryFrame`: a small proof packet with selected memories, conflicts, evidence, trust, validity, source hashes, and coverage counts.

## Why This Matters

Similarity alone can retrieve a bad memory:

- an old policy that has expired
- a low-trust chat note
- a dev-only fact in production context
- another tenant's private memory
- prompt-injection-like text saved as evidence

L5M's hot path is designed to keep those records out of normal answers before semantic scoring happens.

