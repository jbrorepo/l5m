# L5M Scorecard: mempalace-longmemeval

## Raw Retrieval

| Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 450 | 0.5349 | 0.8772 | 0.9387 | 0.8404 | 0.8660 | 0.8819 | 0.0178 |

## Hot Retrieval Latency

| P50 ns | P95 ns | P99 ns | P99.9 ns | Avg Candidates | Avg Scored | Avg Returned | Avg Tokens |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 87427400 | 90726300 | 92488200 | 97400800 | 47.77 | 47.77 | 10.00 | 19357.88 |

## Build And Size

| Build/Load ns | Segment Bytes |
| ---: | ---: |
| 0 | 0 |

## Gate Violations

- violating rows: 0
- raw retrieval only: true
