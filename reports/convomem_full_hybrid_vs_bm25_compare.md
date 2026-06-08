# L5M Benchmark Comparison

| Run | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall | P50 | P95 | P99 | P99.9 | Avg Candidates | Avg Scored | Avg Returned | Avg Tokens | Index Build ns | Segment Bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| runs/convomem_full_hybrid_parent.jsonl | 75336 | 0.3260 | 0.7490 | 0.8134 | 0.5942 | 0.6263 | 0.5553 | 0.0416 | 1305400 | 3871600 | 5585400 | 8238300 | 66.01 | 66.01 | 2.91 | 77.71 | 1415024378400 | 5380550342 |
| runs/convomem_full_bm25.jsonl | 75336 | 0.3323 | 0.5343 | 0.6259 | 0.4637 | 0.4991 | 0.4842 | 0.1566 | 1920000 | 4778400 | 6676400 | 8362900 | 66.17 | 66.17 | 7.72 | 186.96 | 0 | 0 |

## Per-Category Breakdown

### runs/convomem_full_hybrid_parent.jsonl

| Category | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| abstention-evidence | 14910 | 0.8696 | 0.8696 | 0.8696 | 0.8696 | 0.8696 | 0.8696 |
| assistant-facts-evidence | 12745 | 0.3631 | 0.7969 | 0.8363 | 0.6472 | 0.6672 | 0.6070 |
| changing-evidence | 18323 | 0.0935 | 0.6768 | 0.8321 | 0.4547 | 0.5317 | 0.4118 |
| implicit-connection-evidence | 7546 | 0.0560 | 0.6996 | 0.7167 | 0.4441 | 0.4526 | 0.3467 |
| preference-evidence | 5079 | 0.0841 | 0.8031 | 0.8031 | 0.5368 | 0.5368 | 0.4417 |
| user-evidence | 16733 | 0.2633 | 0.6902 | 0.7723 | 0.5464 | 0.5874 | 0.5216 |

### runs/convomem_full_bm25.jsonl

| Category | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| abstention-evidence | 14910 | 0.8696 | 0.8696 | 0.8696 | 0.8696 | 0.8696 | 0.8696 |
| assistant-facts-evidence | 12745 | 0.3739 | 0.6991 | 0.7787 | 0.5706 | 0.6011 | 0.5551 |
| changing-evidence | 18323 | 0.1001 | 0.3579 | 0.5031 | 0.2807 | 0.3397 | 0.3432 |
| implicit-connection-evidence | 7546 | 0.0597 | 0.2025 | 0.3150 | 0.1434 | 0.1827 | 0.1667 |
| preference-evidence | 5079 | 0.0909 | 0.2999 | 0.4501 | 0.1969 | 0.2454 | 0.1838 |
| user-evidence | 16733 | 0.2725 | 0.5241 | 0.6202 | 0.4466 | 0.4855 | 0.4756 |

