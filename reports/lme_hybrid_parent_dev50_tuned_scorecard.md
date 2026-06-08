# L5M Scorecard: mempalace-longmemeval

## Raw Retrieval

| Queries | Recall@1 | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | MRR | Zero Recall |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 50 | 0.4880 | 0.7747 | 0.8710 | 0.7642 | 0.8022 | 0.8099 | 0.0800 |

## Hot Retrieval Latency

| P50 ns | P95 ns | P99 ns | P99.9 ns | Avg Candidates | Avg Scored | Avg Returned | Avg Tokens |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 24672800 | 33858800 | 41378600 | 41378600 | 46.40 | 46.40 | 10.00 | 19256.50 |

## Build And Size

| Build/Load ns | Segment Bytes |
| ---: | ---: |
| 15977223300 | 47993066 |

## Gate Violations

- violating rows: 0
- raw retrieval only: true
