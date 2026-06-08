# L5M Proof Report

## Verdict

- accuracy parity: PASS
- latency lead (end-to-end, build-inclusive): FAIL
- safety gates: PASS
- same config/data/split identity: PASS

## Headline

`runs/hybrid_lme_dev_top10.jsonl` vs `runs/chroma_lme_dev_top10.jsonl`: R@1 0.4280 vs 0.3463, R@5 0.9127 vs 0.8203, R@10 0.9810 vs 0.9350; end-to-end P50 1175018500 ns vs 1112219500 ns (0.95x), end-to-end P95 1302018700 ns vs 1235990600 ns (0.95x); gate violations 0.

> Amortized hot-retrieval P50 (pre-built segment, excludes per-query build): 111533300 ns vs 110155300 ns (0.99x). Only valid when the corpus is built once and queried many times.

## Raw Metrics

Latency columns: E2E = end-to-end build-inclusive (headline); Hot = retrieval-only against a pre-built segment (amortized).

| Run | Queries | R@1 | R@5 | R@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall | E2E P50 ns | E2E P95 ns | E2E P99 ns | Hot P50 ns | Hot P95 ns | Hot P99 ns |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| runs/hybrid_lme_dev_top10.jsonl | 50 | 0.4280 | 0.9127 | 0.9810 | 0.8325 | 0.8602 | 0.8500 | 0.0000 | 1175018500 | 1302018700 | 1332144400 | 111533300 | 119467900 | 143197600 |
| runs/chroma_lme_dev_top10.jsonl | 50 | 0.3463 | 0.8203 | 0.9350 | 0.7379 | 0.7804 | 0.7676 | 0.0200 | 1112219500 | 1235990600 | 1257729100 | 110155300 | 118175800 | 142026000 |

## Identity

| Field | Candidate | Baseline |
| --- | --- | --- |
| config_hash |  |  |
| dataset_hash | cd766d50fe982186db24cea5d73ffaccdda7e0fc1e6eac52bc1318898b4ad7f2 | cd766d50fe982186db24cea5d73ffaccdda7e0fc1e6eac52bc1318898b4ad7f2 |
| split_hash | 99be92a4158f9fc31e37e9272547d9b9e15f056a31fc37d4306c7cc4c567aba7 | 99be92a4158f9fc31e37e9272547d9b9e15f056a31fc37d4306c7cc4c567aba7 |
