# Benchmarks

L5M reports raw retrieval separately from final LLM answer quality. Do not overclaim answer quality from retrieval scores.

## Local Safety Benchmark

This benchmark requires no external datasets. It generates synthetic capsules with wrong tenants, future/expired validity windows, low-trust notes, restricted policy masks, and mixed contexts.

```bash
cargo run -p l5m-bench -- --synthetic-capsules 10000 --synthetic-queries 64 --iterations 1000
```

Report:

| Metric | Value |
| --- | --- |
| p50 retrieval ns | fill from run |
| p95 retrieval ns | fill from run |
| p99 retrieval ns | fill from run |
| avg candidate count before scoring | fill from run |
| avg returned capsule count | fill from run |

## Public Retrieval Benchmarks

Use frozen configs and local-only datasets:

```bash
cargo run -p l5m-benchmarks -- longmemeval --input data/longmemeval_s_cleaned.json --mode hybrid-parent --top-k 10 --held-out --config-file configs/benchmark/longmemeval.json --split-file runs/lme_split_seed42.json --out runs/lme_hybrid_parent_heldout.jsonl

cargo run -p l5m-benchmarks -- locomo --input data/locomo10.json --mode hybrid-parent --granularity session --top-k 10 --config-file configs/benchmark/locomo.json --out runs/locomo_hybrid_parent.jsonl

cargo run -p l5m-benchmarks -- convomem --input data/ConvoMem --mode hybrid-parent --categories all --config-file configs/benchmark/convomem.json --out runs/convomem_hybrid_parent.jsonl
```

Generate scorecards:

```bash
cargo run -p l5m-benchmarks -- scorecard --run runs/lme_hybrid_parent_heldout.jsonl --preset mempalace-longmemeval --out reports/lme_scorecard.md
cargo run -p l5m-benchmarks -- safety --run runs/lme_hybrid_parent_heldout.jsonl --out reports/lme_safety.md
```

## Headline Table Template

| Benchmark | Mode | Recall@10 | NDCG@10 | MRR | Zero Recall | P95 ns | Candidates | Gate Violations | Config Hash | Dataset Hash | Split Hash |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| LongMemEval held-out | hybrid-parent | fill | fill | fill | fill | fill | fill | fill | fill | fill | fill |
| LoCoMo session | hybrid-parent | fill | fill | fill | fill | fill | fill | fill | fill | fill | n/a |
| ConvoMem | hybrid-parent | fill | fill | fill | fill | fill | fill | fill | fill | fill | n/a |

