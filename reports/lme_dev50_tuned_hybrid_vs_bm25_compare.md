# L5M Benchmark Comparison

| Run | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall | P50 | P95 | P99 | P99.9 | Avg Candidates | Avg Scored | Avg Returned | Avg Tokens | Index Build ns | Segment Bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| runs/lme_hybrid_parent_dev50_tuned.jsonl | 50 | 0.4880 | 0.7747 | 0.8710 | 0.7642 | 0.8022 | 0.8099 | 0.0800 | 24672800 | 33858800 | 41378600 | 41378600 | 46.40 | 46.40 | 10.00 | 19256.50 | 15977223300 | 47993066 |
| runs/lme_bm25_dev50_productized.jsonl | 50 | 0.4880 | 0.7747 | 0.8710 | 0.7642 | 0.8022 | 0.8099 | 0.0800 | 87285300 | 89424100 | 90103700 | 90103700 | 47.44 | 47.44 | 10.00 | 19173.40 | 0 | 0 |

## Per-Category Breakdown

### runs/lme_hybrid_parent_dev50_tuned.jsonl

| Category | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| knowledge-update | 8 | 0.3750 | 0.8125 | 0.8750 | 0.7730 | 0.8003 | 0.7812 |
| multi-session | 18 | 0.3278 | 0.7259 | 0.8361 | 0.7391 | 0.7885 | 0.8426 |
| single-session-assistant | 3 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| single-session-preference | 3 | 0.0000 | 0.0000 | 0.6667 | -0.0000 | 0.2075 | 0.0810 |
| single-session-user | 10 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| temporal-reasoning | 8 | 0.3125 | 0.7708 | 0.8125 | 0.7153 | 0.7362 | 0.7292 |

### runs/lme_bm25_dev50_productized.jsonl

| Category | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| knowledge-update | 8 | 0.3750 | 0.8125 | 0.8750 | 0.7730 | 0.8003 | 0.7812 |
| multi-session | 18 | 0.3278 | 0.7259 | 0.8361 | 0.7391 | 0.7885 | 0.8426 |
| single-session-assistant | 3 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| single-session-preference | 3 | 0.0000 | 0.0000 | 0.6667 | -0.0000 | 0.2075 | 0.0810 |
| single-session-user | 10 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| temporal-reasoning | 8 | 0.3125 | 0.7708 | 0.8125 | 0.7153 | 0.7362 | 0.7292 |

