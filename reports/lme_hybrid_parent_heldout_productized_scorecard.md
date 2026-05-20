# L5M Scorecard: mempalace-longmemeval

## Raw Retrieval

| Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 450 | 0.5244 | 0.8789 | 0.9387 | 0.8374 | 0.8625 | 0.8746 | 0.0178 |

## Hot Retrieval Latency

| P50 ns | P95 ns | P99 ns | P99.9 ns | Avg Candidates | Avg Scored | Avg Returned | Avg Tokens |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 25759000 | 39948100 | 45492700 | 54525500 | 46.46 | 46.46 | 10.00 | 19554.10 |

## Build And Size

| Build/Load ns | Segment Bytes |
| ---: | ---: |
| 145710567100 | 432173648 |

## Gate Violations

- violating rows: 0
- raw retrieval only: true
