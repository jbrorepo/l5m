# L5M Productization

L5M now exposes a local Rust SDK and CLI path for agent memory. The product path keeps compiled memory in binary mmap-backed segments and uses typed query requests instead of ad hoc probe assembly.

## SDK

Use `MemoryStore` for product retrieval:

```rust
use l5m_core::{MemoryStore, QueryRequest, RetrievalMode};

let store = MemoryStore::open_segments([".l5m/segments/session.segment"])?;
let response = store.query(&QueryRequest {
    query: "When is the deploy freeze?".to_string(),
    tenant_id: 1,
    as_of: 1_800_000_000,
    context_mask: "0x1".to_string(),
    policy_mask: "0xffff".to_string(),
    trust_floor: 4,
    max_capsules: 8,
    max_tokens: 1024,
    include_supporting: false,
    include_contradictions: false,
    max_hops: 1,
    mode: RetrievalMode::L5m,
})?;
```

`QueryResponse` includes the proof-bearing `MemoryFrame`, retrieval mode, config hash, segment metadata, and total retrieval latency. The existing `retrieve(segment, probe)` API remains available for compatibility.

## CLI

Initialize local state:

```powershell
cargo run -p l5m-cli -- init --dir .l5m
```

Compile typed product memories into multi-view segments:

```powershell
cargo run -p l5m-cli -- ingest --input memories.jsonl --out .l5m/segments --epoch 1
```

Query with a typed JSON request:

```powershell
cargo run -p l5m-cli -- query --segment .l5m/segments/session.segment --request query.json
```

Serve an agent over newline-delimited JSON:

```powershell
cargo run -p l5m-cli -- serve-stdio --segment .l5m/segments
```

Each stdin line is a `QueryRequest`; each stdout line is a `QueryResponse`.

## Python

Python integrations should call the stdio server instead of reimplementing
segment parsing. The repository includes a no-dependency example:

```powershell
cargo run -p l5m-cli -- compile --input examples/seed_memories.json --output target/l5m.segment --epoch 1
python examples/python_stdio_client.py
```

See `docs/python-integration.md` for the request schema and a reusable client
snippet.

## MCP

Use MCP stdio when integrating L5M with agent clients that speak JSON-RPC tool calls:

```powershell
cargo run -p l5m-cli -- mcp-stdio --segment .l5m/segments
```

The server exposes `l5m_query`, `l5m_inspect`, and `l5m_validate`. `l5m_query`
accepts the same `QueryRequest` shape as the CLI and returns `QueryResponse`.

## Benchmark Proof

Frozen configs live under `configs/benchmark/`. Held-out LongMemEval runs must pass `--config-file configs/benchmark/longmemeval.json`; mutable or dev-labelled configs are rejected for held-out reporting.

Run rows include `config_hash`, `dataset_hash`, and `split_hash`, so reports can identify exactly what was measured. Use `diagnose` to bucket misses:

```powershell
cargo run -p l5m-benchmarks -- diagnose --run runs/lme_hybrid_parent_top10.jsonl --out reports/lme_diagnose.md
```

The benchmark path still reports raw retrieval separately from rerank or answer quality, keeps hard gates before scoring, and records gate violation fields for every row.
