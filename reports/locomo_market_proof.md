# L5M Proof Report

## Verdict

- accuracy parity: PASS
- latency lead: PASS
- safety gates: PASS
- same config/data/split identity: PASS

## Headline

`runs/locomo_hybrid_parent_session_top10_tuned.jsonl` vs `runs/locomo_bm25_session_top10_tuned_config.jsonl`: R@1 0.5687 vs 0.5685, R@5 0.8157 vs 0.8148, R@10 0.8939 vs 0.8925; P50 5470900 ns vs 17276600 ns (3.16x), P95 7317600 ns vs 19205300 ns (2.62x); gate violations 0.

## Raw Metrics

| Run | Queries | R@1 | R@5 | R@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall | P50 ns | P95 ns | P99 ns |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| runs/locomo_hybrid_parent_session_top10_tuned.jsonl | 1986 | 0.5687 | 0.8157 | 0.8939 | 0.7231 | 0.7512 | 0.7232 | 0.0624 | 5470900 | 7317600 | 8384100 |
| runs/locomo_bm25_session_top10_tuned_config.jsonl | 1986 | 0.5685 | 0.8148 | 0.8925 | 0.7221 | 0.7502 | 0.7219 | 0.0645 | 17276600 | 19205300 | 20075900 |

## Identity

| Field | Candidate | Baseline |
| --- | --- | --- |
| config_hash | 788b8591e79c86d3ea271f41356ac9bb1d8be023823088e0948a8d429ae6e48a | 788b8591e79c86d3ea271f41356ac9bb1d8be023823088e0948a8d429ae6e48a |
| dataset_hash | 282cde5689a523eb2bf58d37d95c1f1fece99bb687d3ddae7918311b93b04249 | 282cde5689a523eb2bf58d37d95c1f1fece99bb687d3ddae7918311b93b04249 |
| split_hash |  |  |
