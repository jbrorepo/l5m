"""Generic agent-tool shape for L5M admissible memory retrieval."""

from l5m_client import L5MClient


class MemoryTool:
    def __init__(self, segment: str) -> None:
        self.client = L5MClient(segment=segment)

    def __call__(self, query: str) -> dict:
        return self.client.query(
            {
                "query": query,
                "tenant_id": 1,
                "as_of": 1770000000,
                "context_mask": "0x1",
                "policy_mask": "0x1",
                "trust_floor": 4,
                "max_capsules": 8,
                "max_tokens": 1024,
                "mode": "L5m",
            }
        )

    def close(self) -> None:
        self.client.close()
