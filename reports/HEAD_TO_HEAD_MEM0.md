# Head-to-head: L5M vs Mem0 (OSS) — LongMemEval dev-50

**Status: methodology locked; results section populated from the live run.**

## Why this comparison

Mem0 is the most-cited OSS memory layer for AI agents (2026 comparisons rank
it "best overall"). This is the comparison evaluators ask for first. We run it
with the same honest-harness discipline as every other L5M number: one scoring
path for all systems, no system-specific metric code, build costs reported.

## Methodology (identical contract for every system)

1. `l5m-benchmarks export-items` emits, per question, the query and its
   document set (one document = one conversation session) — LongMemEval-S,
   dev split, seed-42, n=50.
2. Each system produces only a **ranking** (`rankings.jsonl`).
3. `l5m-benchmarks external-run` — the same Rust code that scores L5M and
   BM25 — computes every metric. There is no Python scoring path that could
   be tuned to favor either side.

### Mem0 configuration (`bench/mem0_peer.py`)

The real OSS pipeline, not a reimplementation: `mem0.Memory.add()` per session
(LLM fact extraction + mem0's update/dedup logic + embed + index), then
`mem0.Memory.search()` (embed + vector search + mem0 rerank).

| Component | Choice | Rationale |
|---|---|---|
| Extraction LLM | `claude-haiku-4-5` | Same tier as mem0's documented gpt-4o-mini-class defaults; arguably stronger |
| Embedder | all-MiniLM-L6-v2 (fastembed/ONNX) | **Identical embedding model to our Chroma peer** — embedding quality held constant |
| Vector store | Chroma (local) | Same store as the vector-DB peer |
| Ranking derivation | memories carry `capsule_id` metadata from their source session; ranked by first appearance in mem0's search order | Measures whether mem0's pipeline surfaces the right source content |

### Honest caveats, stated up front

- **Mem0 is an extraction-based memory system**: it stores LLM-distilled facts,
  not raw sessions. Retrieval quality therefore depends on what its extractor
  kept — that is the product working as designed, and it is also the risk this
  benchmark measures: if the extractor didn't keep the fact, no search can
  find it.
- Mem0's build cost includes LLM inference (network); L5M's and Chroma's are
  local CPU. The orders-of-magnitude build gap is architectural, not noise.
- dev-50 (n=50): smaller than our n=450 held-out runs, so confidence
  intervals are wide; we report bootstrap CIs and paired significance and do
  not claim more than they support.
- Single run, one extraction-LLM choice. Stronger/weaker extractors will move
  mem0's numbers; we picked a fair mid-tier and state it.

## Results (LongMemEval dev-50, top-k 10)

**Run 1 (2026-06-10): completed, then withheld — and the reason matters.**

The pipeline ran end-to-end (50/50 items, ~51 memories extracted per item,
metadata verified intact by direct inspection of the stored vectors). Mem0
scored R@10 = 0.067 under source-id recall — *below the ~0.20 random
baseline*. A below-random score is a protocol artifact, not a quality
measurement, so we investigated instead of publishing:

**Finding: source-id recall is structurally unfair to consolidation-based
memory.** Mem0's update logic merges facts from later sessions into existing
memories, which retain the *original* session's `capsule_id`. When the answer
appears in session 40, its fact is frequently attached to a memory created
from session 3 — the content is retrievable, but the source attribution is
gone. That is Mem0 working as designed (consolidation is the feature), and it
means the session-id-recall metric we use for ranking-based systems (L5M,
BM25, Chroma — all scored fine under it) cannot fairly score Mem0.

We could have shipped "Mem0: 0.067" with a straight face and a reproducible
harness. It would have been misleading. This project's benchmark policy is
that numbers must measure what they claim to measure.

**Run 2 (planned): QA accuracy — the protocol Mem0's own evaluations use.**
Each system retrieves its top-k context; the same answerer model generates an
answer from that context; the same judge scores it against gold. This measures
what both systems actually promise (answer the question from memory) without
requiring source attribution. The ingested Mem0 stores from Run 1 are
persisted and reusable, so Run 2 costs only the answer+judge calls.

Interim, defensible observations from Run 1 (these do not depend on the
disputed metric):

| Observation | Value |
|---|---|
| Mem0 ingestion cost (dev-50, ~2,650 sessions) | ~50 LLM calls/question; P50 **309 s/question** wall-clock build |
| L5M ingestion (same data, same machine) | P50 **88 ms/question** (no inference) — ~3,500× less build latency |
| Mem0 search latency (post-build) | P50 52 ms |
| L5M query latency (post-build) | P50 1.8 ms |
| Compression behavior | ~53 sessions → ~51 memories/question; source attribution not preserved across consolidation |

The ingestion-cost gap is architectural and uncontested: extraction-based
memory pays LLM inference per write; L5M pays none.

## Reproduce

```bash
./target/release/l5m-benchmarks export-items \
  --input data/longmemeval_s_cleaned.json --benchmark longmemeval \
  --split-file runs/lme_split_seed42.json --dev-only --out runs/_items_lme_dev.jsonl

ANTHROPIC_API_KEY=... python bench/mem0_peer.py \
  --items runs/_items_lme_dev.jsonl --out runs/_rank_mem0_lme_dev.jsonl --top-k 10 --workers 6

./target/release/l5m-benchmarks external-run \
  --input data/longmemeval_s_cleaned.json --benchmark longmemeval \
  --split-file runs/lme_split_seed42.json --dev-only \
  --rankings runs/_rank_mem0_lme_dev.jsonl \
  --mode-label mem0-haiku45-minilm --top-k 10 --out runs/mem0_lme_dev_top10.jsonl

python scripts/analyze_runs.py runs/l5m_hybrid_embed_lme_dev.jsonl "L5M" \
  runs/mem0_lme_dev_top10.jsonl "Mem0"
```
