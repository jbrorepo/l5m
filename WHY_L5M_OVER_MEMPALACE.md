# Why L5M Over MemPalace?

MemPalace helped make local agent memory concrete: store source text, organize it, and retrieve the relevant past context. That is useful. L5M competes on a different axis: **admissible memory**.

L5M does not ask only, "What past text is similar?" It asks, "What memory is relevant, allowed, current, trusted, and provable before it reaches the model?"

## The Core Difference

MemPalace-style systems optimize recall. L5M optimizes memory admission control before recall.

That difference matters when a wrong memory can leak a tenant secret, revive stale policy, treat an untrusted note as truth, or put prompt-injection-like content into an agent's context.

## Direct Comparison

| Question | MemPalace-style recall | L5M admissible recall |
| --- | --- | --- |
| What is the primary job? | Retrieve relevant local memory | Admit only memory that passes hard gates, then rank it |
| What happens before scoring? | Retrieval/index logic | Tenant, context, policy, temporal, and trust gates |
| What is returned? | Context/memory | Proof-bearing `MemoryFrame` |
| How are stale memories handled? | Retrieval-dependent | Temporal validity and conflict metadata |
| How are low-trust notes handled? | Application-dependent | Trust floor before scoring |
| How are tenant leaks prevented? | Application-dependent | Tenant gate before scoring |
| Hot path storage | Typically vector/search stores | Compiled memory-mapped segment |

## When L5M Wins

Use L5M when the agent must not see memory just because it sounds relevant:

- customer support memory with tenant boundaries
- internal copilots using current policy over old chat notes
- security assistants that must separate dev, lab, and production context
- compliance-sensitive agents that need source hashes and evidence
- local agent runtimes that cannot depend on hosted memory or vector databases

## What L5M Does Not Claim

L5M is not claiming that every memory workload should avoid MemPalace, vector search, graph memory, or hosted memory APIs. If you need broad personal recall, automatic extraction, or a full hosted memory product, those systems may be a better fit.

L5M's claim is narrower and sharper:

> Similarity search is not a memory policy. Production agents need admissible memory.

