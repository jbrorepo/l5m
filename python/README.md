# l5m-client

Stdlib-only Python wrapper for querying the local L5M Rust stdio server.

```bash
python -m pip install -e python
```

```python
from l5m_client import L5MClient

with L5MClient(segment="target/l5m.segment") as client:
    response = client.query({
        "query": "How long do we retain production database backups?",
        "tenant_id": 1,
        "as_of": 1770000000,
        "context_mask": "0x1",
        "policy_mask": "0x1",
        "trust_floor": 4,
        "max_capsules": 8,
        "max_tokens": 1024,
        "mode": "L5m",
    })
    print(response["frame"]["capsules"][0]["claim"])
```
