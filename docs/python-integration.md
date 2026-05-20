# Python Integration

L5M does not put Python in the hot retrieval path. Python callers should keep
retrieval in the local Rust process and communicate over newline-delimited JSON
with `l5m serve-stdio`.

## Try It

From the repository root:

```bash
cargo run -p l5m-cli -- compile --input examples/seed_memories.json --output target/l5m.segment --epoch 1
python examples/python_stdio_client.py
```

Expected output includes:

```text
Production database backups are retained for 35 days.
```

## Request Schema

Each stdin line sent to `serve-stdio` is a JSON `QueryRequest`:

```json
{
  "query": "How long do we retain production database backups?",
  "tenant_id": 1,
  "as_of": 1770000000,
  "context_mask": "0x1",
  "policy_mask": "0x1",
  "trust_floor": 4,
  "max_capsules": 8,
  "max_tokens": 1024,
  "include_supporting": false,
  "include_contradictions": true,
  "max_hops": 1,
  "mode": "L5m"
}
```

The response line is a JSON `QueryResponse` with:

- `frame`: proof-bearing `MemoryFrame` containing selected capsules, conflicts,
  source hashes, trust, validity, and coverage counts.
- `mode`: retrieval mode used by the Rust store.
- `config_hash`: deterministic hash of the request configuration.
- `segment_metadata`: paths, epochs, tenants, and capsule counts.
- `total_retrieval_ns`: measured retrieval time in nanoseconds.

## Minimal Client

```python
import json
import subprocess

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

process = subprocess.Popen(
    [
        "cargo",
        "run",
        "-q",
        "-p",
        "l5m-cli",
        "--",
        "serve-stdio",
        "--segment",
        "target/l5m.segment",
    ],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    text=True,
)

stdout, _ = process.communicate(json.dumps(request) + "\n", timeout=30)
response = json.loads(stdout.splitlines()[0])
print(response["frame"]["capsules"][0]["claim"])
```

For production use, start the process once and keep its stdin/stdout open for
multiple query lines instead of spawning it per request.
