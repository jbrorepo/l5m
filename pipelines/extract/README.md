# Memory extraction pipeline — transcript → capsules

Turns conversation transcripts into L5M memory capsules with provenance,
offline. This is the ingest-side complement to L5M's zero-inference-on-the-
hot-path design: extraction (the expensive, model-shaped work) happens in
batch; retrieval stays deterministic and sub-millisecond.

## Two modes

| Mode | Dependencies | What it catches |
|---|---|---|
| `rules` (default) | **None** (stdlib only; runs in CI) | Explicit durable facts: "remember that…", decisions, "my X is Y" attributes, preferences, dated facts |
| `llm` | `pip install anthropic` + `ANTHROPIC_API_KEY` | Everything rules catch plus paraphrases, implications, multi-turn context — extracted by Claude (`claude-opus-4-8`) with schema-guaranteed structured output |

## Usage

```bash
# Deterministic extraction
python extract.py meeting.txt --tenant 7 -o capsules.json

# Claude-based extraction
python extract.py meeting.txt --mode llm --tenant 7 -o capsules.json

# Then ingest, any of:
l5m-cli compile --input capsules.json --output memories.segment --epoch 1
curl -X POST localhost:8080/v1/memories -H 'x-l5m-api-key: …' -H 'x-l5m-tenant: 7' \
     -H 'content-type: application/json' -d @<(jq '.[0]' capsules.json)
# or clients/python: c.insert(capsule) per capsule
```

Transcript format: plain text, one utterance per line, `Speaker: text`.

## Design properties

- **Idempotent re-ingestion.** `capsule_id = SHA-256(tenant, claim, source)`
  truncated to u128 — re-running the pipeline on the same transcript produces
  the same ids, so ingestion is an upsert, never a duplicate flood.
- **Provenance.** `evidence` is the verbatim utterance; `source_uri` is
  `file#Lline` in rules mode. Every memory is traceable to who said it, where.
- **Speaker attribution.** First-person fragments ("my email is…") are
  rewritten to name the speaker, so claims stay meaningful outside the
  conversation.
- **Trust levels.** Rules assign 6-8 by pattern confidence; the LLM rates 0-10
  (clamped). Recall-time `trust_floor` then gates accordingly — low-confidence
  extractions can be stored but excluded from high-stakes contexts.
- **Tenant scoping.** Capsules are stamped with `--tenant`; on HTTP ingest the
  server additionally forces the authenticated tenant.

## Verified end-to-end

CI runs the deterministic suite (extraction correctness, chitchat exclusion,
speaker attribution, capsule shape, id determinism, LLM-mode request/response
handling against a fake client). The output is verified compatible with
`l5m-cli compile` → gated query.

```bash
python -m pytest pipelines/extract -q
```
