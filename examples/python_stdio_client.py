#!/usr/bin/env python3
"""Query L5M from Python through the local Rust stdio server.

Run from the repository root after compiling the example segment:

    cargo run -p l5m-cli -- compile --input examples/seed_memories.json --output target/l5m.segment --epoch 1
    python examples/python_stdio_client.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "python"))
from l5m_client import L5MClient

ROOT = Path(__file__).resolve().parents[1]
SEGMENT = ROOT / "target" / "l5m.segment"


def main() -> int:
    request = {
        "query": "How long do we retain production database backups?",
        "tenant_id": 1,
        "as_of": 1770000000,
        "context_mask": "0x1",
        "policy_mask": "0x1",
        "trust_floor": 4,
        "max_capsules": 8,
        "max_tokens": 1024,
        "include_contradictions": True,
        "mode": "L5m",
    }

    with L5MClient(segment=SEGMENT, cwd=ROOT) as client:
        response = client.query(request)
    capsules = response["frame"]["capsules"]
    for capsule in capsules:
        print(capsule["claim"])
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:
        print(f"error: {exc}", file=sys.stderr)
        raise SystemExit(1)
