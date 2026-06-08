# L5M Scorecard: mempalace-convomem

## Raw Retrieval

| Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 75336 | 0.3323 | 0.5343 | 0.6259 | 0.4637 | 0.4991 | 0.4842 | 0.1566 |

## Hot Retrieval Latency

| P50 ns | P95 ns | P99 ns | P99.9 ns | Avg Candidates | Avg Scored | Avg Returned | Avg Tokens |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1920000 | 4778400 | 6676400 | 8362900 | 66.17 | 66.17 | 7.72 | 186.96 |

## Build And Size

| Build/Load ns | Segment Bytes |
| ---: | ---: |
| 0 | 0 |

## Gate Violations

- violating rows: 0
- raw retrieval only: true
