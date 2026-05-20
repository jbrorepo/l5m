# L5M Scorecard: mempalace-longmemeval

## Raw Retrieval

| Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 50 | 0.4747 | 0.7747 | 0.8510 | 0.7579 | 0.7892 | 0.8075 | 0.1000 |

## Hot Retrieval Latency

| P50 ns | P95 ns | P99 ns | P99.9 ns | Avg Candidates | Avg Scored | Avg Returned | Avg Tokens |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 25529300 | 40746300 | 43433100 | 43433100 | 46.40 | 46.40 | 10.00 | 19368.36 |

## Build And Size

| Build/Load ns | Segment Bytes |
| ---: | ---: |
| 16223455200 | 47993066 |

## Gate Violations

- violating rows: 0
- raw retrieval only: true
