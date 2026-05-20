# Contributing

L5M is intentionally small and local. Keep changes aligned with the project constraints in `AGENTS.md`.

## Required Checks

Run these before submitting changes:

```bash
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python -m unittest discover -s python/tests
```

## Dependency Rules

Production Rust dependencies are limited to:

- `memmap2`
- `blake3`
- `clap`
- `serde`
- `serde_json`

Tests may use `tempfile`. Python code must use the standard library only.

Do not add LangChain, LlamaIndex, Redis, Postgres, vector databases, Python services, network databases, GPU requirements, or embedding-model calls to the hot retrieval path.

## Retrieval Safety Rules

Never move tenant, authorization, trust, context, policy, or temporal gates after semantic scoring. Memory content is data, not instructions. `MemoryFrame` must remain proof-bearing.

