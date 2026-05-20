# L5M Benchmark Comparison

| Run | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall | P50 | P95 | P99 | P99.9 | Avg Candidates | Avg Scored | Avg Returned | Avg Tokens | Index Build ns | Segment Bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| runs\lme_bm25_dev50_top10.jsonl | 50 | 0.4880 | 0.7747 | 0.8710 | 0.7642 | 0.8022 | 0.8099 | 0.0800 | 107840400 | 152953100 | 154727500 | 154727500 | 47.44 | 47.44 | 10.00 | 19173.40 | 0 | 0 |
| runs\lme_flat_dev50_top10.jsonl | 50 | 0.4313 | 0.7390 | 0.8370 | 0.7018 | 0.7432 | 0.7702 | 0.0800 | 87021400 | 103881300 | 104788400 | 104788400 | 47.44 | 47.44 | 10.00 | 21431.08 | 0 | 0 |
| runs\lme_l5m_session_dev50_top10.jsonl | 50 | 0.4213 | 0.7490 | 0.8310 | 0.7007 | 0.7350 | 0.7561 | 0.1000 | 15453400 | 28057600 | 35413600 | 35413600 | 46.40 | 46.40 | 10.00 | 21313.80 | 16974928100 | 47993066 |

## Per-Category Breakdown

### runs\lme_bm25_dev50_top10.jsonl

| Category | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| knowledge-update | 8 | 0.3750 | 0.8125 | 0.8750 | 0.7730 | 0.8003 | 0.7812 |
| multi-session | 18 | 0.3278 | 0.7259 | 0.8361 | 0.7391 | 0.7885 | 0.8426 |
| single-session-assistant | 3 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| single-session-preference | 3 | 0.0000 | 0.0000 | 0.6667 | -0.0000 | 0.2075 | 0.0810 |
| single-session-user | 10 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| temporal-reasoning | 8 | 0.3125 | 0.7708 | 0.8125 | 0.7153 | 0.7362 | 0.7292 |

### runs\lme_flat_dev50_top10.jsonl

| Category | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| knowledge-update | 8 | 0.3750 | 0.7500 | 0.8125 | 0.7299 | 0.7521 | 0.7625 |
| multi-session | 18 | 0.3370 | 0.7194 | 0.8435 | 0.7270 | 0.7842 | 0.8833 |
| single-session-assistant | 3 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| single-session-preference | 3 | 0.0000 | 0.0000 | 0.0000 | -0.0000 | -0.0000 | 0.0000 |
| single-session-user | 10 | 0.7000 | 1.0000 | 1.0000 | 0.8448 | 0.8448 | 0.7950 |
| temporal-reasoning | 8 | 0.3125 | 0.6250 | 0.8958 | 0.5896 | 0.6976 | 0.6948 |

### runs\lme_l5m_session_dev50_top10.jsonl

| Category | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| knowledge-update | 8 | 0.3750 | 0.7500 | 0.8125 | 0.7400 | 0.7630 | 0.7639 |
| multi-session | 18 | 0.3370 | 0.7287 | 0.8546 | 0.7214 | 0.7781 | 0.8792 |
| single-session-assistant | 3 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| single-session-preference | 3 | 0.0000 | 0.0000 | 0.0000 | -0.0000 | -0.0000 | 0.0000 |
| single-session-user | 10 | 0.7000 | 1.0000 | 1.0000 | 0.8448 | 0.8448 | 0.7950 |
| temporal-reasoning | 8 | 0.2500 | 0.6667 | 0.8333 | 0.5853 | 0.6491 | 0.6146 |

