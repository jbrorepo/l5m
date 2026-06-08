# L5M Benchmark Comparison

Latency: E2E* = end-to-end build-inclusive; Hot* = retrieval-only against a pre-built segment (amortized).

| Run | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall | E2E P50 | E2E P95 | E2E P99 | Hot P50 | Hot P95 | Hot P99 | Avg Candidates | Avg Scored | Avg Returned | Avg Tokens | Index Build ns | Segment Bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| runs/l5m_lme_dev_top10.jsonl | 50 | 0.4880 | 0.7747 | 0.8710 | 0.7642 | 0.8022 | 0.8099 | 0.0800 | 66799500 | 74415300 | 172740100 | 1390400 | 1773500 | 1948600 | 46.40 | 46.40 | 10.00 | 19256.50 | 3355953000 | 47993066 |
| runs/bm25_lme_dev_top10.jsonl | 50 | 0.4880 | 0.7747 | 0.8710 | 0.7642 | 0.8022 | 0.8099 | 0.0800 | 12669200 | 13643900 | 14718600 | 12669200 | 13643900 | 14718600 | 47.44 | 47.44 | 10.00 | 19173.40 | 0 | 0 |
| runs/chroma_lme_dev_top10.jsonl | 50 | 0.3463 | 0.8203 | 0.9350 | 0.7379 | 0.7804 | 0.7676 | 0.0200 | 1112219500 | 1235990600 | 1257729100 | 110155300 | 118175800 | 142026000 | 47.44 | 47.44 | 10.00 | 18249.46 | 50546244500 | 0 |

## Per-Category Breakdown

### runs/l5m_lme_dev_top10.jsonl

| Category | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| knowledge-update | 8 | 0.3750 | 0.8125 | 0.8750 | 0.7730 | 0.8003 | 0.7812 |
| multi-session | 18 | 0.3278 | 0.7259 | 0.8361 | 0.7391 | 0.7885 | 0.8426 |
| single-session-assistant | 3 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| single-session-preference | 3 | 0.0000 | 0.0000 | 0.6667 | -0.0000 | 0.2075 | 0.0810 |
| single-session-user | 10 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| temporal-reasoning | 8 | 0.3125 | 0.7708 | 0.8125 | 0.7153 | 0.7362 | 0.7292 |

### runs/bm25_lme_dev_top10.jsonl

| Category | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| knowledge-update | 8 | 0.3750 | 0.8125 | 0.8750 | 0.7730 | 0.8003 | 0.7812 |
| multi-session | 18 | 0.3278 | 0.7259 | 0.8361 | 0.7391 | 0.7885 | 0.8426 |
| single-session-assistant | 3 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| single-session-preference | 3 | 0.0000 | 0.0000 | 0.6667 | -0.0000 | 0.2075 | 0.0810 |
| single-session-user | 10 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| temporal-reasoning | 8 | 0.3125 | 0.7708 | 0.8125 | 0.7153 | 0.7362 | 0.7292 |

### runs/chroma_lme_dev_top10.jsonl

| Category | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| knowledge-update | 8 | 0.2500 | 0.8125 | 0.8750 | 0.7117 | 0.7390 | 0.7292 |
| multi-session | 18 | 0.3046 | 0.8620 | 0.9861 | 0.8229 | 0.8761 | 0.8889 |
| single-session-assistant | 3 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| single-session-preference | 3 | 0.6667 | 1.0000 | 1.0000 | 0.8770 | 0.8770 | 0.8333 |
| single-session-user | 10 | 0.2000 | 0.6000 | 0.9000 | 0.4393 | 0.5339 | 0.4211 |
| temporal-reasoning | 8 | 0.3542 | 0.8750 | 0.8750 | 0.7959 | 0.7959 | 0.8542 |

