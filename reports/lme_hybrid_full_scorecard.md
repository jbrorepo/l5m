# L5M Scorecard: mempalace-longmemeval

## Raw Retrieval

| Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 500 | 0.5195 | 0.8685 | 0.9300 | 0.8295 | 0.8552 | 0.8679 | 0.0260 |

## Hot Retrieval Latency

| P50 ns | P95 ns | P99 ns | P99.9 ns | Avg Candidates | Avg Scored | Avg Returned | Avg Tokens |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 26507000 | 43092300 | 49569600 | 62303200 | 46.45 | 46.45 | 10.00 | 19535.53 |

## Build And Size

| Build/Load ns | Segment Bytes |
| ---: | ---: |
| 170058124800 | 480166714 |

## Gate Violations

- violating rows: 0
- raw retrieval only: true
