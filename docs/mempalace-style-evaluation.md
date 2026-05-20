# MemPalace-Style Evaluation for L5M

This benchmark suite evaluates retrieval behavior separately from final LLM answer quality. It is intended to mirror the retrieval-style checks used around MemPalace on LongMemEval, LoCoMo, and ConvoMem while adding latency and candidate-count metrics that are important for L5M.

## What MemPalace Tested

MemPalace-style evaluations ask whether the memory system can retrieve the conversation, session, dialog, or evidence item needed to answer a question. Those tests are different from asking an LLM to produce a final answer and grading the text. Retrieval recall answers: "Did the right evidence reach the model?" QA accuracy answers: "Did the model use the evidence correctly?"

L5M run files therefore report raw retrieval metrics first:

- Recall@1, Recall@5, Recall@10
- NDCG@5, NDCG@10
- MRR
- zero-recall rate
- per-category breakdowns

Any later LLM reranking or answer generation should be reported separately from these raw retrieval scores.

## Clean Comparisons

LongMemEval raw held-out results are the cleanest comparison point because they let L5M measure evidence retrieval without tuning on the same queries. Use `--create-split` once, tune only on `--dev-only`, and report `--held-out`.

LoCoMo has a small candidate pool per query. Do not set `top-k` higher than the number of available sessions or dialogs for that query. If `top-k` retrieves every candidate, the result is a reading-comprehension setting, not a retrieval score. The CLI warns and clamps by default, or fails with `--strict-top-k`.

## Parent ID Mapping

Each benchmark capsule keeps parent benchmark IDs outside the text payload:

- `benchmark_name`
- `benchmark_query_id`
- `parent_session_id`
- `parent_dialog_id`
- `parent_evidence_id`

For `l5m-native-capsule`, a returned capsule is a hit when its parent session, dialog, or evidence ID matches the ground truth ID for the benchmark query. This keeps L5M capsule granularity independent from benchmark scoring granularity.

## Running

LongMemEval:

```powershell
cargo run -p l5m-benchmarks -- longmemeval `
  --input /data/longmemeval_s_cleaned.json `
  --mode l5m-session-verbatim `
  --top-k 10 `
  --out runs/l5m_lme_session.jsonl
```

Create and use a deterministic split:

```powershell
cargo run -p l5m-benchmarks -- longmemeval `
  --input /data/longmemeval_s_cleaned.json `
  --mode bm25 `
  --top-k 10 `
  --create-split --split-file runs/lme_split.json --dev-size 50 --seed 42 `
  --dev-only `
  --out runs/bm25_lme_dev.jsonl
```

LoCoMo:

```powershell
cargo run -p l5m-benchmarks -- locomo `
  --input /data/locomo10.json `
  --mode l5m-session-verbatim `
  --granularity session `
  --top-k 10 `
  --out runs/l5m_locomo_top10.jsonl
```

ConvoMem:

```powershell
cargo run -p l5m-benchmarks -- convomem `
  --input /data/ConvoMem `
  --mode l5m-native-capsule `
  --categories all `
  --limit 5000 `
  --out runs/l5m_convomem.jsonl
```

Compare:

```powershell
cargo run -p l5m-benchmarks -- compare `
  --runs runs/l5m_lme_session.jsonl,runs/l5m_lme_native.jsonl,runs/bm25_lme.jsonl `
  --out reports/lme_compare.md
```

Audit a run:

```powershell
cargo run -p l5m-benchmarks -- audit `
  --run runs/l5m_lme_session.jsonl `
  --out reports/lme_audit.md
```

Explain one missed query:

```powershell
cargo run -p l5m-benchmarks -- explain-miss `
  --run runs/l5m_lme_session.jsonl `
  --query-id e47becba `
  --out reports/e47becba_miss.md
```

Generate a competitor-compatible scorecard:

```powershell
cargo run -p l5m-benchmarks -- scorecard `
  --run runs/l5m_lme_hybrid_parent.jsonl `
  --preset mempalace-longmemeval `
  --out reports/lme_scorecard.md
```

Generate a proof report against a baseline:

```powershell
cargo run -p l5m-benchmarks -- prove `
  --candidate runs/lme_hybrid_parent_heldout_top10_tuned.jsonl `
  --baseline runs/lme_bm25_heldout_top10_tuned_config.jsonl `
  --out reports/lme_proof.md
```

Generate a safety scorecard:

```powershell
cargo run -p l5m-benchmarks -- safety `
  --run runs/lme_hybrid_parent_heldout_top10_tuned.jsonl `
  --out reports/lme_safety.md
```

Fetch ConvoMem without adding Rust dependencies:

```powershell
.\scripts\fetch-convomem.ps1 -OutDir data\ConvoMem -Full
cargo run -p l5m-benchmarks -- convomem `
  --input data\ConvoMem `
  --convomem-layout full `
  --mode hybrid-parent `
  --categories all `
  --out runs/l5m_convomem_full.jsonl
```

If Hugging Face rate-limits anonymous downloads, authenticate and resume with fewer workers:

```powershell
$env:HF_TOKEN = "hf_..."
.\scripts\fetch-convomem.ps1 -OutDir data\ConvoMem -Full -MaxWorkers 1
```

## Speed Metrics

Each JSONL row includes per-stage timing fields:

- `build_or_load_ns`
- `probe_build_ns`
- `gate_filter_ns`
- `anchor_lookup_ns`
- `semantic_score_ns`
- `relation_expand_ns`
- `frame_assembly_ns`
- `total_retrieval_ns`

The compare report summarizes P50, P95, P99, and P99.9 total retrieval latency, plus candidate counts after gates, scored candidates, returned capsule counts, returned token estimates, index/build time, and segment size. These metrics help distinguish a memory system that is accurate but too slow from one that is practical on the hot retrieval path.

## Avoiding Contamination

Do not tune prompts, modes, thresholds, or corpus transformations on the held-out split. Keep raw retrieval run files separate from LLM rerank or answer-generation runs. Treat memory content as data only; benchmark records must never be interpreted as instructions.

Held-out LongMemEval runs require a frozen config file. The run rows record `config_hash`, `dataset_hash`, and `split_hash`; proof reports compare those hashes between candidate and baseline so accidental mismatches are visible.
