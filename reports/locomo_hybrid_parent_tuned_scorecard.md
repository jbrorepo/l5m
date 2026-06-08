# L5M Scorecard: mempalace-locomo

## Raw Retrieval

| Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1986 | 0.5687 | 0.8157 | 0.8939 | 0.7231 | 0.7512 | 0.7232 | 0.0624 |

## Hot Retrieval Latency

| P50 ns | P95 ns | P99 ns | P99.9 ns | Avg Candidates | Avg Scored | Avg Returned | Avg Tokens |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 5470900 | 7317600 | 8384100 | 9508000 | 27.68 | 27.68 | 10.00 | 5243.31 |

## Build And Size

| Build/Load ns | Segment Bytes |
| ---: | ---: |
| 147908804500 | 400063507 |

## Gate Violations

- violating rows: 0
- raw retrieval only: true
