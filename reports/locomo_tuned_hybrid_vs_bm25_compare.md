# L5M Benchmark Comparison

| Run | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall | P50 | P95 | P99 | P99.9 | Avg Candidates | Avg Scored | Avg Returned | Avg Tokens | Index Build ns | Segment Bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| runs/locomo_hybrid_parent_session_top10_tuned.jsonl | 1986 | 0.5687 | 0.8157 | 0.8939 | 0.7231 | 0.7512 | 0.7232 | 0.0624 | 5470900 | 7317600 | 8384100 | 9508000 | 27.68 | 27.68 | 10.00 | 5243.31 | 147908804500 | 400063507 |
| runs/locomo_bm25_session_top10.jsonl | 1986 | 0.5685 | 0.8148 | 0.8925 | 0.7221 | 0.7502 | 0.7219 | 0.0645 | 19143000 | 33114000 | 33582300 | 37610400 | 27.70 | 27.70 | 10.00 | 5234.01 | 0 | 0 |

## Per-Category Breakdown

### runs/locomo_hybrid_parent_session_top10_tuned.jsonl

| Category | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| adversarial | 446 | 0.7175 | 0.9081 | 0.9619 | 0.8270 | 0.8446 | 0.8067 |
| open-domain | 841 | 0.6688 | 0.9102 | 0.9649 | 0.8039 | 0.8217 | 0.7758 |
| single-hop | 282 | 0.1781 | 0.4925 | 0.6682 | 0.4483 | 0.5215 | 0.5803 |
| temporal | 321 | 0.5421 | 0.8292 | 0.9024 | 0.7073 | 0.7319 | 0.6804 |
| temporal-inference | 96 | 0.2361 | 0.4637 | 0.5911 | 0.3910 | 0.4391 | 0.4384 |

### runs/locomo_bm25_session_top10.jsonl

| Category | Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| adversarial | 446 | 0.7152 | 0.9081 | 0.9596 | 0.8262 | 0.8430 | 0.8052 |
| open-domain | 841 | 0.6688 | 0.9114 | 0.9637 | 0.8043 | 0.8213 | 0.7756 |
| single-hop | 282 | 0.1734 | 0.4900 | 0.6701 | 0.4428 | 0.5180 | 0.5706 |
| temporal | 321 | 0.5483 | 0.8229 | 0.8993 | 0.7071 | 0.7329 | 0.6828 |
| temporal-inference | 96 | 0.2361 | 0.4611 | 0.5874 | 0.3897 | 0.4365 | 0.4396 |

