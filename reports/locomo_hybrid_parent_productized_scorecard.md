# L5M Scorecard: mempalace-locomo

## Raw Retrieval

| Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1986 | 0.5627 | 0.8139 | 0.8936 | 0.7177 | 0.7462 | 0.7174 | 0.0629 |

## Hot Retrieval Latency

| P50 ns | P95 ns | P99 ns | P99.9 ns | Avg Candidates | Avg Scored | Avg Returned | Avg Tokens |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 5320200 | 7099600 | 8239800 | 11152200 | 27.68 | 27.68 | 10.00 | 5342.84 |

## Build And Size

| Build/Load ns | Segment Bytes |
| ---: | ---: |
| 142685277200 | 400063507 |

## Gate Violations

- violating rows: 0
- raw retrieval only: true
