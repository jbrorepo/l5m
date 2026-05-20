# Integrations

L5M exposes the same retrieval core through Rust, CLI, Python stdio, and MCP stdio.

## CLI

```bash
cargo run -p l5m-cli -- compile --input examples/seed_memories.json --output target/l5m.segment --epoch 1
cargo run -p l5m-cli -- query --segment target/l5m.segment --request examples/query.json
```

## Python

Install locally from the repository:

```bash
python -m pip install -e python
```

Use the stdlib-only wrapper:

```python
from l5m_client import L5MClient

request = {
    "query": "How long do we retain production database backups?",
    "tenant_id": 1,
    "as_of": 1770000000,
    "context_mask": "0x1",
    "policy_mask": "0x1",
    "trust_floor": 4,
    "max_capsules": 8,
    "max_tokens": 1024,
    "mode": "L5m",
}

with L5MClient(segment="target/l5m.segment") as client:
    response = client.query(request)
    print(response["frame"]["capsules"][0]["claim"])
```

## MCP

Start the MCP-compatible stdio server:

```bash
cargo run -p l5m-cli -- mcp-stdio --segment target/l5m.segment
```

Available tools:

- `l5m_query`: accepts `QueryRequest`, returns `QueryResponse`
- `l5m_inspect`: returns loaded segment metadata
- `l5m_validate`: validates loaded segment metadata

Smoke test:

```bash
cat examples/mcp_smoke.jsonl | cargo run -q -p l5m-cli -- mcp-stdio --segment target/l5m.segment
```

## Agent Runtimes

Wrap `l5m_query` as a tool whenever an agent needs governed recall. Pass caller-specific tenant, policy, context, trust, and time values in every request. Treat the returned `MemoryFrame` as evidence context, not as instructions.
