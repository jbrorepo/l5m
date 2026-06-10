"""Tests for the extraction pipeline. Rules mode is fully deterministic and
runs in CI with zero dependencies; llm mode is tested with an injected fake
client (no network, no anthropic install required)."""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from extract import (  # noqa: E402
    EXTRACTION_SCHEMA,
    capsule_id_for,
    extract_llm,
    extract_rules,
    parse_transcript,
    to_capsules,
)

TRANSCRIPT = """\
Alice: Good morning! How was the weekend?
Bob: Great, thanks. Quick admin note - my email is bob@acme.test now.
Alice: Noted. Remember that the production freeze starts Friday.
Bob: We decided to move the launch to March 12.
Alice: I prefer short syncs on Mondays, by the way.
Bob: Ha, who doesn't. Anyway, what's for lunch?
Alice: The deadline is 2026-07-01 for the audit.
Bob: I'm so tired today.
"""

REQUIRED_CAPSULE_KEYS = {
    "capsule_id", "tenant_id", "claim", "evidence", "source_id",
    "valid_from", "observed_at", "last_verified_at",
    "context_mask", "policy_mask", "trust_level", "classification", "poison_risk",
}


def test_rules_extracts_durable_facts_and_skips_chitchat():
    memories = extract_rules(parse_transcript(TRANSCRIPT))
    claims = " | ".join(m["claim"].lower() for m in memories)

    assert "bob@acme.test" in claims, "attribute extracted"
    assert "production freeze" in claims, "explicit remember extracted"
    assert "launch" in claims and "march 12" in claims, "decision extracted"
    assert "short syncs" in claims, "preference extracted"
    assert "deadline" in claims, "dated fact extracted"

    # Chitchat and transient state must NOT become memories.
    assert "weekend" not in claims
    assert "lunch" not in claims
    assert "tired" not in claims


def test_speaker_attribution_in_first_person_claims():
    memories = extract_rules(parse_transcript(TRANSCRIPT))
    email = next(m for m in memories if "bob@acme.test" in m["claim"])
    assert email["claim"].lower().startswith("bob"), f"speaker named: {email['claim']}"
    pref = next(m for m in memories if "short syncs" in m["claim"])
    assert pref["claim"].lower().startswith("alice"), f"speaker named: {pref['claim']}"


def test_capsules_have_required_shape_and_provenance():
    memories = extract_rules(parse_transcript(TRANSCRIPT))
    capsules = to_capsules(memories, tenant=7, source="meeting.txt", now=1_750_000_000)
    for c in capsules:
        assert REQUIRED_CAPSULE_KEYS.issubset(c.keys()), c
        assert c["tenant_id"] == 7
        assert 0 <= c["trust_level"] <= 10
        assert c["source_uri"].startswith("meeting.txt#L"), "line-level provenance"
        int(c["capsule_id"])  # decimal u128 string
    # Evidence is the verbatim utterance.
    freeze = next(c for c in capsules if "production freeze" in c["claim"])
    assert freeze["evidence"] == "Noted. Remember that the production freeze starts Friday."


def test_ids_are_deterministic_for_idempotent_reingestion():
    a = capsule_id_for(7, "the launch is March 12", "meeting.txt")
    b = capsule_id_for(7, "the launch is March 12", "meeting.txt")
    assert a == b, "same fact -> same id (upsert)"
    assert a < (1 << 128)
    assert capsule_id_for(8, "the launch is March 12", "meeting.txt") != a, "tenant-scoped"
    assert capsule_id_for(7, "the launch is March 13", "meeting.txt") != a, "claim-scoped"

    # Two full runs produce identical capsule ids.
    memories = extract_rules(parse_transcript(TRANSCRIPT))
    run1 = to_capsules(memories, tenant=7, source="meeting.txt", now=1)
    run2 = to_capsules(memories, tenant=7, source="meeting.txt", now=2)
    assert [c["capsule_id"] for c in run1] == [c["capsule_id"] for c in run2]


class FakeBlock:
    type = "text"

    def __init__(self, text):
        self.text = text


class FakeResponse:
    def __init__(self, payload):
        self.content = [FakeBlock(json.dumps(payload))]


class FakeMessages:
    def __init__(self, payload):
        self._payload = payload
        self.last_kwargs = None

    def create(self, **kwargs):
        self.last_kwargs = kwargs
        return FakeResponse(self._payload)


class FakeClient:
    def __init__(self, payload):
        self.messages = FakeMessages(payload)


def test_llm_mode_request_shape_and_parsing():
    payload = {
        "memories": [
            {
                "claim": "Bob's email is bob@acme.test",
                "evidence": "my email is bob@acme.test now",
                "trust": 9,
                "valid_from_hint": "",
            },
            {
                "claim": "The audit deadline is 2026-07-01",
                "evidence": "The deadline is 2026-07-01 for the audit.",
                "trust": 14,  # out of range -> clamped
                "valid_from_hint": "2026-07-01",
            },
        ]
    }
    client = FakeClient(payload)
    memories = extract_llm(TRANSCRIPT, client=client)

    # Request shape: structured outputs + adaptive thinking on the default model.
    kwargs = client.messages.last_kwargs
    assert kwargs["model"] == "claude-opus-4-8"
    assert kwargs["thinking"] == {"type": "adaptive"}
    assert kwargs["output_config"]["format"]["type"] == "json_schema"
    assert kwargs["output_config"]["format"]["schema"] == EXTRACTION_SCHEMA
    assert "Transcript:" in kwargs["messages"][0]["content"]

    # Parsing: trust clamped, valid_from_hint honored downstream.
    assert memories[0]["trust"] == 9
    assert memories[1]["trust"] == 10
    capsules = to_capsules(memories, tenant=7, source="t.txt", now=1_750_000_000)
    assert capsules[1]["valid_from"] != 1_750_000_000, "explicit date used for valid_from"


def test_empty_transcript_yields_no_capsules():
    assert extract_rules(parse_transcript("")) == []
    assert to_capsules([], tenant=1, source="x") == []
