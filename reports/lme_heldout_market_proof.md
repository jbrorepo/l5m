# L5M Proof Report

## Verdict

- accuracy parity: PASS
- latency lead: PASS
- safety gates: PASS
- same config/data/split identity: PASS

## Headline

`runs/lme_hybrid_parent_heldout_top10_tuned.jsonl` vs `runs/lme_bm25_heldout_top10_tuned_config.jsonl`: R@1 0.5349 vs 0.5349, R@5 0.8772 vs 0.8772, R@10 0.9387 vs 0.9387; P50 27069500 ns vs 91516300 ns (3.38x), P95 41685000 ns vs 95943200 ns (2.30x); gate violations 0.

## Raw Metrics

| Run | Queries | R@1 | R@5 | R@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall | P50 ns | P95 ns | P99 ns |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| runs/lme_hybrid_parent_heldout_top10_tuned.jsonl | 450 | 0.5349 | 0.8772 | 0.9387 | 0.8401 | 0.8658 | 0.8811 | 0.0178 | 27069500 | 41685000 | 47625000 |
| runs/lme_bm25_heldout_top10_tuned_config.jsonl | 450 | 0.5349 | 0.8772 | 0.9387 | 0.8404 | 0.8660 | 0.8819 | 0.0178 | 91516300 | 95943200 | 99337900 |

## Identity

| Field | Candidate | Baseline |
| --- | --- | --- |
| config_hash | 9e938e58edd13fab2342cefa012b490d19877796e836a835330660fbf7e0bcd2 | 9e938e58edd13fab2342cefa012b490d19877796e836a835330660fbf7e0bcd2 |
| dataset_hash | cd766d50fe982186db24cea5d73ffaccdda7e0fc1e6eac52bc1318898b4ad7f2 | cd766d50fe982186db24cea5d73ffaccdda7e0fc1e6eac52bc1318898b4ad7f2 |
| split_hash | 99be92a4158f9fc31e37e9272547d9b9e15f056a31fc37d4306c7cc4c567aba7 | 99be92a4158f9fc31e37e9272547d9b9e15f056a31fc37d4306c7cc4c567aba7 |
