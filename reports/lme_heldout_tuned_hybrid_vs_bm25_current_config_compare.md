# L5M Benchmark Comparison

| Run | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall | P50 | P95 | P99 | P99.9 | Avg Candidates | Avg Scored | Avg Returned | Avg Tokens | Index Build ns | Segment Bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| runs/lme_hybrid_parent_heldout_top10_tuned.jsonl | 450 | 0.5349 | 0.8772 | 0.9387 | 0.8401 | 0.8658 | 0.8811 | 0.0178 | 27069500 | 41685000 | 47625000 | 56988000 | 46.46 | 46.46 | 10.00 | 19361.60 | 152779371700 | 432173648 |
| runs/lme_bm25_heldout_top10_tuned_config.jsonl | 450 | 0.5349 | 0.8772 | 0.9387 | 0.8404 | 0.8660 | 0.8819 | 0.0178 | 91516300 | 95943200 | 99337900 | 109995200 | 47.77 | 47.77 | 10.00 | 19357.88 | 0 | 0 |

## Per-Category Breakdown

### runs/lme_hybrid_parent_heldout_top10_tuned.jsonl

| Category | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| knowledge-update | 70 | 0.4714 | 0.9643 | 0.9857 | 0.9432 | 0.9514 | 0.9619 |
| multi-session | 115 | 0.3530 | 0.7906 | 0.9019 | 0.7700 | 0.8210 | 0.8843 |
| single-session-assistant | 53 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| single-session-preference | 27 | 0.4074 | 0.7407 | 0.8889 | 0.5793 | 0.6300 | 0.5486 |
| single-session-user | 60 | 0.8500 | 0.9833 | 1.0000 | 0.9197 | 0.9248 | 0.9005 |
| temporal-reasoning | 125 | 0.4168 | 0.8347 | 0.9017 | 0.7970 | 0.8248 | 0.8450 |

### runs/lme_bm25_heldout_top10_tuned_config.jsonl

| Category | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| knowledge-update | 70 | 0.4714 | 0.9643 | 0.9857 | 0.9432 | 0.9514 | 0.9619 |
| multi-session | 115 | 0.3530 | 0.7906 | 0.9019 | 0.7703 | 0.8211 | 0.8843 |
| single-session-assistant | 53 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| single-session-preference | 27 | 0.4074 | 0.7407 | 0.8889 | 0.5793 | 0.6300 | 0.5486 |
| single-session-user | 60 | 0.8500 | 0.9833 | 1.0000 | 0.9190 | 0.9240 | 0.8996 |
| temporal-reasoning | 125 | 0.4168 | 0.8347 | 0.9017 | 0.7983 | 0.8260 | 0.8482 |

