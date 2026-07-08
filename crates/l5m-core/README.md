# l5m-core

The core engine of [L5M](https://github.com/jbrorepo/l5m): a security-gated
memory/retrieval library where authorization gates (tenant, context, policy,
trust, temporal) run on the candidate set **before** relevance scoring — so an
unauthorized memory is never even a retrieval candidate.

```rust
use l5m_core::{Segment, MemoryProbe, retrieve};

let segment = Segment::open("memories.segment")?;
let probe = MemoryProbe::build(
    "How long do we retain backups?",
    /* tenant */ 7, /* as_of */ 1_770_000_000,
    /* context_mask */ 0xffff, /* policy_mask */ 0xffff, /* trust_floor */ 4,
);
let frame = retrieve(&segment, &probe)?;
# Ok::<(), l5m_core::L5mError>(())
```

## Highlights

- **Gate-before-scoring** authorization, proven by a `proptest` invariant over
  randomized multi-tenant corpora and adversarial bypass tests.
- **Memory-mapped columnar segments** + LSH candidate generation:
  sub-millisecond gated retrieval at 1M memories / 1,000 tenants.
- **Real-time writes**: LSM delta (amortized O(1)), fsync'd WAL durability,
  automatic compaction.
- **Bi-temporal**: `valid_from`/`valid_until`/`observed_at` with `as_of`
  point-in-time recall.
- Memory-safe: `#![deny(unsafe_code)]` with a single documented, fuzzed mmap.
- Optional `encryption` feature (ChaCha20-Poly1305 sealed segments).

Full project, benchmarks, and the reviewer's guide:
<https://github.com/jbrorepo/l5m>.

## License

MIT OR Apache-2.0
