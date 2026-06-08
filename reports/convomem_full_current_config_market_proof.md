# L5M Proof Report

## Verdict

- accuracy parity: PASS
- latency lead: PASS
- safety gates: PASS
- same config/data/split identity: PASS

## Headline

`runs/convomem_full_hybrid_parent_current_config.jsonl` vs `runs/convomem_full_bm25_current_config.jsonl`: R@1 0.3328 vs 0.3323, R@5 0.7490 vs 0.5343, R@10 0.8134 vs 0.6259; P50 1300400 ns vs 1870800 ns (1.44x), P95 3537800 ns vs 4662500 ns (1.32x); gate violations 0.

## Raw Metrics

| Run | Queries | R@1 | R@5 | R@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall | P50 ns | P95 ns | P99 ns |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| runs/convomem_full_hybrid_parent_current_config.jsonl | 75336 | 0.3328 | 0.7490 | 0.8134 | 0.5969 | 0.6290 | 0.5600 | 0.0416 | 1300400 | 3537800 | 5099200 |
| runs/convomem_full_bm25_current_config.jsonl | 75336 | 0.3323 | 0.5343 | 0.6259 | 0.4637 | 0.4991 | 0.4842 | 0.1566 | 1870800 | 4662500 | 6502300 |

## Identity

| Field | Candidate | Baseline |
| --- | --- | --- |
| config_hash | dffc705336d4c2cf792f90b19ae4a3e83b4e96b528452d455e43c0e97d06fe04 | dffc705336d4c2cf792f90b19ae4a3e83b4e96b528452d455e43c0e97d06fe04 |
| dataset_hash | af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262 | af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262 |
| split_hash |  |  |
