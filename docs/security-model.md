# L5M Security Model

L5M treats memory content as data, never as instructions. Capsules can contain hostile text, but retrieval should not execute it, follow it, or promote it above hard gates.

## Tenant Isolation

Segments are tenant-scoped in the MVP. Retrieval starts from the loaded tenant segment, and returned normal capsules must match the probe tenant.

## Policy Gating

Each capsule has a `policy_mask`; each caller supplies a `caller_policy_mask`. A capsule is eligible only when the masks intersect. This prevents unauthorized capsules from reaching semantic scoring.

## Trust Floor

Each capsule has a `trust_level`. The caller supplies a `trust_floor`, and capsules below that floor are removed before semantic scoring. Low-trust notes therefore cannot outrank approved policy.

## Temporal Validity

Capsules must satisfy `valid_from <= as_of` and `valid_until` absent or `valid_until >= as_of`. Superseded capsules are also excluded from normal answers when a visible current capsule supersedes them.

## Memory Poisoning Defense

Poisoned or quarantined memories should be assigned low trust, restrictive policy masks, high poison-risk flags, or all three. These controls operate before scoring and again as score penalties if a capsule remains eligible.

## Prompt Injection Handling

Prompt-injection-like memory is stored only as evidence data. It is not interpreted as an instruction. The example corpus includes a quarantined capsule that says to ignore previous instructions; high trust-floor retrieval excludes it.

## Conflict Metadata

Contradicted, expired, or superseded records may be returned only when explicitly requested and are placed in `MemoryFrame.conflicts` with relation notes. They are not returned as normal answer capsules unless they pass all hard gates.
