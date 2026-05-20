# L5M Architecture

L5M is a retrieval lattice for small structured memory capsules. It is designed for local low-latency use beside an LLM server, without a networked database, external embedding service, GPU, or Python hot-path service.

## 5D Lattice

Each `MemoryCapsule` carries coordinates in five dimensions:

- Semantic coordinate: anchor terms, entity-like IDs, a deterministic 256-bit semantic fingerprint, and a 64-element int8 residual vector.
- Temporal coordinate: `valid_from`, optional `valid_until`, `observed_at`, `last_verified_at`, and relation-based supersession.
- Context coordinate: tenant plus bit masks for environment, project, user group, sensitivity, and task type.
- Relation coordinate: edges for support, contradiction, supersession, dependency, derivation, and duplication.
- Veracity coordinate: trust level, source ID, policy mask, content/source hashes, classification, and poison-risk flag.

## Gates Before Scoring

Retrieval starts with all capsules in the segment and applies hard gates before semantic scoring:

1. Context mask gate.
2. Policy mask gate.
3. Temporal validity and supersession gate.
4. Trust floor gate.
5. Anchor/entity/semantic candidate narrowing.
6. Scoring and top-N selection.

No normal answer capsule may bypass tenant, policy, trust, temporal, or context gates. Expired or superseded capsules may appear only as explicitly requested conflict/supersession metadata.

## Immutable Memory-Mapped Segments

The compiler turns JSON fixtures into immutable binary segment files. Retrieval opens the segment with `memmap2`, validates magic/version/hash, parses fixed metadata and payload offsets, and builds lookup indexes in memory. The hot path performs no JSON parsing and no network calls.

Immutable segments make deployment simple: generate a new epoch, validate it, then atomically swap the file used by a future LLM server integration.

## MemoryFrame Output

`MemoryFrame` is the prompt-facing output object. It includes:

- Epoch and query hash.
- Selected `FrameCapsule` claims and evidence.
- Trust, validity, source ID, source hash, and score.
- Conflict/supersession/support notes when requested.
- Coverage counters showing gate and candidate behavior.
