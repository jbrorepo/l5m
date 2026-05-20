# L5M: Admissible Memory For Agents

L5M is a local memory admission layer for agents. It compiles memory into memory-mapped binary segments, applies tenant, context, policy, temporal, and trust gates before semantic ranking, and returns compact proof-bearing `MemoryFrame` objects for agent context.

L5M is not a vector database and not a hosted memory service. Its competitive wedge is admissible recall: relevant, allowed, current, trusted, and provable memory before it reaches the model.

## Repository Layout

- `crates/l5m-core`: segment compiler, memory-mapped loader, retrieval gates, scoring, and product API.
- `crates/l5m-cli`: local compile, ingest, query, and stdio serving commands.
- `crates/l5m-bench`: segment latency benchmark.
- `crates/l5m-benchmarks`: MemPalace-style retrieval benchmark adapters and reports.
- `configs/benchmark`: frozen benchmark configs.
- `docs`: architecture, security, segment format, productization, and launch notes.
- `examples`: tiny local fixtures for compile/query smoke tests.

Large datasets, generated run rows, and vendored external benchmark sources are local-only and ignored by git. See `docs/github-launch.md` for the first-push checklist and benchmark data notes.

## Why L5M?

Most memory systems ask, "What past text is similar?" L5M asks, "What memory is allowed to reach the model?"

Use L5M when you need:

- tenant isolation before retrieval scoring
- stale-policy prevention through validity windows
- trust floors that exclude low-confidence notes
- context and policy masks for prod/dev/lab separation
- proof-bearing output with claims, evidence, source hashes, and conflicts

Do not use L5M as your first choice when you mainly need hosted personalization, automatic memory extraction, or broad consumer memory without governance constraints.

See `WHY_L5M_OVER_MEMPALACE.md` for the direct MemPalace comparison and `docs/admissible-memory.md` for the plain-English model.

## Quickstart

Prerequisite: Rust stable from <https://rustup.rs/>.

```bash
git clone https://github.com/jbrorepo/l5m.git
cd l5m
cargo test --workspace
cargo run -p l5m-cli -- compile --input examples/seed_memories.json --output target/l5m.segment --epoch 1
cargo run -p l5m-cli -- validate --segment target/l5m.segment
cargo run -p l5m-cli -- query --segment target/l5m.segment --request examples/query.json
```

The query prints a `QueryResponse` JSON object with a proof-bearing `frame`.
For the included fixture, the top returned claim should state that production
database backups are retained for 35 days.

Gauntlet demo:

```bash
cargo run -p l5m-cli -- query --segment target/l5m.segment --request examples/query.json
```

Expected headline: L5M returns the current 35-day production policy as a normal capsule, while stale, low-trust, dev-only, restricted, and prompt-injection-like memories stay out of normal answers. See `docs/bad-memory-gauntlet.md`.

## Python Integration

Python applications can use the local CLI over newline-delimited JSON. This
keeps segment loading and retrieval in Rust while avoiding a Python service,
network database, vector store, or embedding dependency.

After compiling `target/l5m.segment` in the quickstart:

```bash
python examples/python_stdio_client.py
```

The example starts `l5m serve-stdio`, sends one `QueryRequest`, reads one
`QueryResponse`, and prints the selected memory claims. See
`docs/python-integration.md` for the request schema and a reusable client
snippet.

## 5D Model

- Semantic: anchors, entity IDs, 256-bit semantic fingerprint, int8 residual vector.
- Temporal: validity window, observation time, verification time, supersession, stale/retracted representation.
- Context: tenant, environment, project, user group, sensitivity, task type encoded as masks.
- Relation: supports, contradicts, supersedes, depends on, derived from, duplicate of.
- Veracity: trust level, source ID/type, verification method, policy mask, content hash, classification, poison risk.

## Build

```bash
cargo build --workspace
```

## Test

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Compile Example Memory

```bash
cargo run -p l5m-cli -- compile --input examples/seed_memories.json --output target/l5m.segment --epoch 1
```

This writes `target/l5m.segment` and `target/l5m.segment.manifest.json`.

## Query

```bash
cargo run -p l5m-cli -- query --segment target/l5m.segment --tenant 1 --query "How long do we retain production database backups?" --as-of 1770000000 --context-mask 0xffff --policy-mask 0xffff --trust-floor 4 --max-capsules 8 --include-contradictions
```

The command prints pretty JSON `MemoryFrame` output with selected capsules, conflicts, source hashes, trust, validity, and coverage counts.

## Product SDK and Agent CLI

For product integrations, prefer `MemoryStore::query` with typed `QueryRequest` input. It returns a `QueryResponse` containing the proof-bearing frame, retrieval mode, config hash, segment metadata, and retrieval latency.

```bash
cargo run -p l5m-cli -- init --dir .l5m
cargo run -p l5m-cli -- ingest --input memories.jsonl --out .l5m/segments --epoch 1
cargo run -p l5m-cli -- query --segment .l5m/segments/session.segment --request query.json
cargo run -p l5m-cli -- serve-stdio --segment .l5m/segments
```

See `docs/productization.md` for the JSONL memory shape, stdio agent contract, and benchmark proof workflow.

## MCP

```bash
cargo run -p l5m-cli -- mcp-stdio --segment target/l5m.segment
```

The MCP-compatible stdio server exposes `l5m_query`, `l5m_inspect`, and `l5m_validate`. Smoke test:

```bash
cat examples/mcp_smoke.jsonl | cargo run -q -p l5m-cli -- mcp-stdio --segment target/l5m.segment
```

## Benchmark

```bash
cargo run -p l5m-bench -- --segment target/l5m.segment --queries examples/queries.json --iterations 1000
```

The benchmark uses `std::time::Instant` and reports p50, p95, p99 retrieval latency, average candidate count before scoring, and average returned capsule count.

## Current Limitations

- The segment loader validates and memory maps the binary file, then materializes capsule strings and lookup indexes in process for MVP simplicity.
- Semantic scoring is deterministic hashing, not model-trained embedding.
- Context and policy are bit masks; richer policy expression is future work.
- Segment index data is summarized in the binary, while lookup maps are rebuilt during load.
- Conflict and supersession handling is one-hop by default.
- No encryption, signatures, or key shredding yet.

## Roadmap

- Native LLM probe head.
- KV Capsules.
- Model-server integration.
- SIMD scoring.
- Signed manifests.
- Encryption and key shredding.
- Better claim extraction.
- Richer relation graph.
