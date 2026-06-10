#!/usr/bin/env python3
"""Transcript -> L5M memory capsules (offline extraction pipeline).

L5M keeps model inference OFF the retrieval hot path by design; this tool is
the matching ingest-side: it runs offline/batch, turning conversation
transcripts into structured memory capsules with provenance, ready for
`l5m-cli compile`, `POST /v1/memories`, or the Python/TS SDKs.

Two modes:

  rules (default)  Deterministic pattern extraction. Zero dependencies,
                   reproducible, runs in CI. Catches explicit, durable facts
                   (preferences, decisions, deadlines, attributes).

  llm              Claude-based extraction (model: claude-opus-4-8) for facts
                   the rules miss — paraphrases, implications, multi-turn
                   context. Requires `pip install anthropic` and
                   ANTHROPIC_API_KEY. Uses structured outputs, so the model's
                   response is schema-guaranteed JSON.

Provenance & idempotence:
  - evidence = the verbatim utterance; source_uri = file#line.
  - capsule_id = SHA-256(tenant, claim, source) truncated to u128, so re-running
    the pipeline on the same transcript produces the SAME ids — re-ingestion is
    an upsert, not a duplicate flood.

Usage:
  python extract.py transcript.txt --tenant 7 -o capsules.json
  python extract.py meeting.txt --mode llm --tenant 7 -o capsules.json

Transcript format: plain text, one utterance per line, "Speaker: text".
Lines without a "Speaker:" prefix are treated as continuation/narration and
skipped.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import time
from dataclasses import dataclass
from typing import Any, Dict, List, Optional

DEFAULT_LLM_MODEL = "claude-opus-4-8"

# ---------------------------------------------------------------------------
# Transcript parsing
# ---------------------------------------------------------------------------


@dataclass
class Utterance:
    line_no: int
    speaker: str
    text: str


SPEAKER_RE = re.compile(r"^\s*([A-Za-z][\w .'-]{0,40}?)\s*:\s*(.+)$")


def parse_transcript(raw: str) -> List[Utterance]:
    utterances = []
    for i, line in enumerate(raw.splitlines(), start=1):
        m = SPEAKER_RE.match(line)
        if m:
            utterances.append(Utterance(i, m.group(1).strip(), m.group(2).strip()))
    return utterances


# ---------------------------------------------------------------------------
# Rules mode — deterministic extraction
# ---------------------------------------------------------------------------

# (pattern, trust, why). Trust reflects how reliably the pattern indicates a
# durable fact; callers can floor it out at recall time.
RULES = [
    # Explicit ask to remember — highest confidence.
    (re.compile(r"\b(?:please )?remember (?:that )?(.{8,200})", re.I), 8, "explicit"),
    # Decisions.
    (re.compile(r"\bwe (?:decided|agreed) (?:that |to )?(.{8,200})", re.I), 7, "decision"),
    # Personal/org attributes: "my X is Y", "our X is Y".
    (
        re.compile(
            r"\b((?:my|our) (?:\w+ ){0,2}(?:name|email|phone|address|birthday|anniversary|"
            r"manager|team|company|timezone|budget|goal|policy) (?:is|are) .{2,120})",
            re.I,
        ),
        7,
        "attribute",
    ),
    # Preferences.
    (
        re.compile(r"\b(i (?:prefer|always|never|usually|like to|hate|love) .{4,150})", re.I),
        6,
        "preference",
    ),
    # Dated facts: deadline/launch/meeting/renewal is <when>.
    (
        re.compile(
            r"\b(the (?:deadline|launch|meeting|renewal|review|migration|audit) "
            r"(?:is|was|will be) .{2,120})",
            re.I,
        ),
        6,
        "dated",
    ),
]


def extract_rules(utterances: List[Utterance]) -> List[Dict[str, Any]]:
    """Deterministic extraction: returns raw memories (claim/evidence/line/trust)."""
    memories = []
    seen_claims = set()
    for utt in utterances:
        for pattern, trust, kind in RULES:
            m = pattern.search(utt.text)
            if not m:
                continue
            claim = normalize_claim(m.group(1), utt.speaker)
            if claim.lower() in seen_claims:
                continue
            seen_claims.add(claim.lower())
            memories.append(
                {
                    "claim": claim,
                    "evidence": utt.text,
                    "line": utt.line_no,
                    "trust": trust,
                    "kind": kind,
                }
            )
            break  # one memory per utterance; first (highest-priority) rule wins
    return memories


def normalize_claim(fragment: str, speaker: str) -> str:
    """Clean a matched fragment into a standalone claim with the speaker named
    (so 'my email is X' stays attributable once it leaves the conversation)."""
    claim = fragment.strip().rstrip(".!,;").strip()
    claim = re.sub(r"\s+", " ", claim)
    # Attribute first-person fragments to the speaker.
    claim = re.sub(r"^(i|my|our|we)\b", lambda m: f"{speaker} ({m.group(1).lower()})", claim, count=1, flags=re.I)
    return claim


# ---------------------------------------------------------------------------
# LLM mode — Claude extraction with structured outputs
# ---------------------------------------------------------------------------

EXTRACTION_SCHEMA: Dict[str, Any] = {
    "type": "object",
    "properties": {
        "memories": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "claim": {
                        "type": "string",
                        "description": "The durable fact, stated plainly in third person.",
                    },
                    "evidence": {
                        "type": "string",
                        "description": "Verbatim quote from the transcript supporting the claim.",
                    },
                    "trust": {
                        "type": "integer",
                        "description": "Confidence 0-10 that this is a durable, correctly-read fact.",
                    },
                    "valid_from_hint": {
                        "type": "string",
                        "description": "If the fact has an explicit start date, ISO date; else empty string.",
                    },
                },
                "required": ["claim", "evidence", "trust", "valid_from_hint"],
                "additionalProperties": False,
            },
        }
    },
    "required": ["memories"],
    "additionalProperties": False,
}

EXTRACTION_PROMPT = """\
Extract durable, long-term memories from this conversation transcript.

Include: stated preferences, decisions, personal/organizational attributes,
deadlines and dates, commitments, and corrections of earlier facts.
Exclude: chitchat, transient state ("I'm tired today"), questions, speculation,
and anything the speakers explicitly retracted.

For each memory: state the claim plainly in third person (name the speaker),
quote the supporting utterance verbatim as evidence, and rate trust 0-10
(10 = explicitly and unambiguously stated).

Transcript:
{transcript}
"""


def extract_llm(
    raw_transcript: str,
    model: str = DEFAULT_LLM_MODEL,
    client: Optional[Any] = None,
) -> List[Dict[str, Any]]:
    """Claude-based extraction. `client` is injectable for tests; by default the
    official Anthropic SDK client is constructed (requires ANTHROPIC_API_KEY)."""
    if client is None:
        try:
            import anthropic  # lazy: only the llm mode needs it
        except ImportError:
            sys.exit("llm mode requires the Anthropic SDK: pip install anthropic")
        client = anthropic.Anthropic()

    response = client.messages.create(
        model=model,
        max_tokens=16000,
        thinking={"type": "adaptive"},
        output_config={"format": {"type": "json_schema", "schema": EXTRACTION_SCHEMA}},
        messages=[
            {
                "role": "user",
                "content": EXTRACTION_PROMPT.format(transcript=raw_transcript),
            }
        ],
    )
    text = next(b.text for b in response.content if b.type == "text")
    data = json.loads(text)

    memories = []
    for m in data.get("memories", []):
        trust = max(0, min(10, int(m.get("trust", 5))))
        memories.append(
            {
                "claim": str(m["claim"]).strip(),
                "evidence": str(m["evidence"]).strip(),
                "line": 0,  # LLM mode quotes evidence; line provenance not tracked
                "trust": trust,
                "kind": "llm",
                "valid_from_hint": str(m.get("valid_from_hint", "")).strip(),
            }
        )
    return memories


# ---------------------------------------------------------------------------
# Capsule assembly
# ---------------------------------------------------------------------------


def capsule_id_for(tenant: int, claim: str, source: str) -> int:
    """Deterministic u128 id: same (tenant, claim, source) -> same id, so
    re-ingestion upserts instead of duplicating."""
    digest = hashlib.sha256(f"{tenant}\x00{claim}\x00{source}".encode("utf-8")).digest()
    return int.from_bytes(digest[:16], "big")


def to_capsules(
    memories: List[Dict[str, Any]],
    tenant: int,
    source: str,
    now: Optional[int] = None,
) -> List[Dict[str, Any]]:
    now = int(time.time()) if now is None else now
    capsules = []
    for m in memories:
        valid_from = now
        hint = m.get("valid_from_hint", "")
        if hint:
            try:
                valid_from = int(time.mktime(time.strptime(hint, "%Y-%m-%d")))
            except ValueError:
                pass
        capsule = {
            "capsule_id": str(capsule_id_for(tenant, m["claim"], source)),
            "tenant_id": tenant,
            "claim": m["claim"],
            "evidence": m["evidence"],
            "source_id": 0,
            "source_uri": f"{source}#L{m['line']}" if m.get("line") else source,
            "valid_from": valid_from,
            "observed_at": now,
            "last_verified_at": now,
            "context_mask": "0xffff",
            "policy_mask": "0xffff",
            "trust_level": m["trust"],
            "classification": 1,
            "poison_risk": 0,
        }
        capsules.append(capsule)
    return capsules


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("transcript", help="Transcript file ('Speaker: text' per line)")
    parser.add_argument("--mode", choices=["rules", "llm"], default="rules")
    parser.add_argument("--tenant", type=int, required=True, help="Tenant id capsules belong to")
    parser.add_argument("--model", default=DEFAULT_LLM_MODEL, help="Claude model for llm mode")
    parser.add_argument("-o", "--output", default="-", help="Output file (default stdout)")
    args = parser.parse_args(argv)

    with open(args.transcript, "r", encoding="utf-8") as f:
        raw = f.read()

    if args.mode == "rules":
        memories = extract_rules(parse_transcript(raw))
    else:
        memories = extract_llm(raw, model=args.model)

    capsules = to_capsules(memories, tenant=args.tenant, source=args.transcript)
    out = json.dumps(capsules, indent=2)
    if args.output == "-":
        print(out)
    else:
        with open(args.output, "w", encoding="utf-8") as f:
            f.write(out + "\n")
        print(f"extracted {len(capsules)} memories -> {args.output}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
