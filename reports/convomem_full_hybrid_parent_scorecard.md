# L5M Scorecard: mempalace-convomem

## Raw Retrieval

| Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 75336 | 0.3260 | 0.7490 | 0.8134 | 0.5942 | 0.6263 | 0.5553 | 0.0416 |

## Hot Retrieval Latency

| P50 ns | P95 ns | P99 ns | P99.9 ns | Avg Candidates | Avg Scored | Avg Returned | Avg Tokens |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1305400 | 3871600 | 5585400 | 8238300 | 66.01 | 66.01 | 2.91 | 77.71 |

## Build And Size

| Build/Load ns | Segment Bytes |
| ---: | ---: |
| 1415024378400 | 5380550342 |

## Gate Violations

- violating rows: 0
- raw retrieval only: true
