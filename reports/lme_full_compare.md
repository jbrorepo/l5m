# L5M Benchmark Comparison

| Run | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall | P50 | P95 | P99 | P99.9 | Avg Candidates | Avg Scored | Avg Returned | Avg Tokens | Index Build ns | Segment Bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| runs\lme_bm25_top10.jsonl | 500 | 0.5302 | 0.8670 | 0.9320 | 0.8328 | 0.8597 | 0.8747 | 0.0240 | 91783300 | 98876600 | 105514300 | 119474400 | 47.73 | 47.73 | 10.00 | 19339.44 | 0 | 0 |
| runs\lme_flat_top10.jsonl | 500 | 0.4384 | 0.8064 | 0.8942 | 0.7472 | 0.7822 | 0.7894 | 0.0560 | 97277300 | 104519100 | 108714400 | 116226800 | 47.73 | 47.73 | 10.00 | 21555.11 | 0 | 0 |
| runs\lme_l5m_session_top10.jsonl | 500 | 0.4432 | 0.8030 | 0.8943 | 0.7465 | 0.7824 | 0.7903 | 0.0480 | 16940100 | 34956300 | 46131800 | 52318100 | 46.45 | 46.45 | 10.00 | 21584.93 | 179030567500 | 480166714 |

## Per-Category Breakdown

### runs\lme_bm25_top10.jsonl

| Category | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| knowledge-update | 78 | 0.4615 | 0.9487 | 0.9744 | 0.9257 | 0.9359 | 0.9434 |
| multi-session | 133 | 0.3496 | 0.7818 | 0.8930 | 0.7661 | 0.8167 | 0.8787 |
| single-session-assistant | 56 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| single-session-preference | 30 | 0.3667 | 0.6667 | 0.8667 | 0.5214 | 0.5878 | 0.5018 |
| single-session-user | 70 | 0.8714 | 0.9857 | 1.0000 | 0.9306 | 0.9349 | 0.9140 |
| temporal-reasoning | 133 | 0.4105 | 0.8308 | 0.8964 | 0.7933 | 0.8206 | 0.8410 |

### runs\lme_flat_top10.jsonl

| Category | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| knowledge-update | 78 | 0.4423 | 0.9103 | 0.9551 | 0.8803 | 0.8973 | 0.9114 |
| multi-session | 133 | 0.3214 | 0.7655 | 0.8645 | 0.7291 | 0.7727 | 0.8310 |
| single-session-assistant | 56 | 0.6964 | 0.8929 | 0.9464 | 0.8065 | 0.8242 | 0.7849 |
| single-session-preference | 30 | 0.1667 | 0.4667 | 0.6333 | 0.3057 | 0.3598 | 0.2762 |
| single-session-user | 70 | 0.7000 | 0.9429 | 0.9714 | 0.8349 | 0.8432 | 0.8014 |
| temporal-reasoning | 133 | 0.3679 | 0.7548 | 0.8842 | 0.7155 | 0.7697 | 0.7875 |

### runs\lme_l5m_session_top10.jsonl

| Category | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| knowledge-update | 78 | 0.4359 | 0.9103 | 0.9487 | 0.8752 | 0.8900 | 0.9043 |
| multi-session | 133 | 0.3321 | 0.7471 | 0.8550 | 0.7226 | 0.7709 | 0.8369 |
| single-session-assistant | 56 | 0.6786 | 0.8929 | 0.9464 | 0.7924 | 0.8097 | 0.7663 |
| single-session-preference | 30 | 0.1333 | 0.4667 | 0.7333 | 0.2943 | 0.3815 | 0.2749 |
| single-session-user | 70 | 0.7286 | 0.9286 | 0.9714 | 0.8399 | 0.8539 | 0.8159 |
| temporal-reasoning | 133 | 0.3792 | 0.7679 | 0.8754 | 0.7285 | 0.7723 | 0.7896 |

