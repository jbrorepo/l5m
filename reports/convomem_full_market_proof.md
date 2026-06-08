# L5M Proof Report

## Verdict

- accuracy parity: FAIL
- latency lead: PASS
- safety gates: PASS
- same config/data/split identity: PASS

## Headline

`runs/convomem_full_hybrid_parent.jsonl` vs `runs/convomem_full_bm25.jsonl`: R@1 0.3260 vs 0.3323, R@5 0.7490 vs 0.5343, R@10 0.8134 vs 0.6259; P50 1305400 ns vs 1920000 ns (1.47x), P95 3871600 ns vs 4778400 ns (1.23x); gate violations 0.

## Raw Metrics

| Run | Queries | R@1 | R@5 | R@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall | P50 ns | P95 ns | P99 ns |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| runs/convomem_full_hybrid_parent.jsonl | 75336 | 0.3260 | 0.7490 | 0.8134 | 0.5942 | 0.6263 | 0.5553 | 0.0416 | 1305400 | 3871600 | 5585400 |
| runs/convomem_full_bm25.jsonl | 75336 | 0.3323 | 0.5343 | 0.6259 | 0.4637 | 0.4991 | 0.4842 | 0.1566 | 1920000 | 4778400 | 6676400 |

## Identity

| Field | Candidate | Baseline |
| --- | --- | --- |
| config_hash | 23f1e436d697da2ddf6287cb469df25d4d4eb90e9f3733673c835b88fcc15b97 | 23f1e436d697da2ddf6287cb469df25d4d4eb90e9f3733673c835b88fcc15b97 |
| dataset_hash | af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262 | af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262 |
| split_hash |  |  |
