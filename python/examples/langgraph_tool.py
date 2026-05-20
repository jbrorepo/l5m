"""LangGraph-style tool wrapper without depending on LangGraph."""

from l5m_client import L5MClient


def l5m_query_tool(question: str) -> str:
    request = {
        "query": question,
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
    return "\n".join(capsule["claim"] for capsule in response["frame"]["capsules"])


if __name__ == "__main__":
    print(l5m_query_tool("How long do we retain production database backups?"))
