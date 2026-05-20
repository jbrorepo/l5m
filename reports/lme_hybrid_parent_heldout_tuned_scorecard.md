# L5M Scorecard: mempalace-longmemeval

## Raw Retrieval

| Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 450 | 0.5349 | 0.8772 | 0.9387 | 0.8401 | 0.8658 | 0.8811 | 0.0178 |

## Hot Retrieval Latency

| P50 ns | P95 ns | P99 ns | P99.9 ns | Avg Candidates | Avg Scored | Avg Returned | Avg Tokens |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 27069500 | 41685000 | 47625000 | 56988000 | 46.46 | 46.46 | 10.00 | 19361.60 |

## Build And Size

| Build/Load ns | Segment Bytes |
| ---: | ---: |
| 152779371700 | 432173648 |

## Gate Violations

- violating rows: 0
- raw retrieval only: true
