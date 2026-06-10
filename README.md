# L5M: Low-Latency 5D Memory for AI

**Authorization enforced *before* relevance — no query can surface a memory it isn't allowed to see.** Memory-safe Rust, a fuzzed untrusted-input parser, and a machine-checked security invariant.

L5M is a local, memory-mapped retrieval engine for LLM/agent memory. Its hard gates (tenant, context, policy, trust, temporal) run on the candidate set *before* any scoring, so authorization never depends on relevance. Hybrid lexical + dense retrieval that is **statistically more accurate than a production vector DB** (see below), with a query hot path in the low single-digit milliseconds and sub-millisecond multi-tenant retrieval at 1M capsules.

> **Honesty note:** earlier drafts of this README contained inflated benchmark
> claims ("3.4× faster", a competitor row with no implementation). Those were
> wrong; the numbers below are independently re-derived, build-inclusive where
> relevant, and significance-tested. See [`VALIDATION_REPORT.md`](VALIDATION_REPORT.md),
> [`SCORE_ANALYSIS.md`](SCORE_ANALYSIS.md), and [`OPTIMIZATION_REPORT.md`](OPTIMIZATION_REPORT.md).

```rust
// Query memory in 5 lines
let segment = Segment::open("memories.segment")?;
let probe = MemoryProbe::build("How long do we retain backups?", tenant_id, timestamp, 0xffff, 0xffff, 4);
let frame = retrieve(&segment, &probe)?;
println!("Found {} capsules with trust ≥ {}", frame.capsules.len(), probe.trust_floor);
```

## Why L5M?

LongMemEval **held-out split (n=450)**, top-k 10, paired bootstrap significance:

| System | R@1 | R@5 | R@10 | MRR | Query latency (p50) | Security gates |
|--------|----:|----:|-----:|----:|--------------------:|----------------|
| **L5M native hybrid** (lexical + dense) | 0.505 | **0.899** | **0.954** | **0.867** | ~1.8 ms | ✅ before scoring |
| Chroma + all-MiniLM-L6-v2 (real vector DB) | 0.474 | 0.861 | 0.926 | 0.832 | ~110 ms | ❌ post-filter |
| L5M lexical only | 0.524 | 0.879 | 0.939 | 0.875 | ~1.4 ms | ✅ before scoring |
| BM25 | 0.524 | 0.879 | 0.939 | 0.875 | n/a | ❌ none |

L5M's native hybrid **beats the vector DB on R@5/R@10/MRR with statistical
significance** (p ≤ 0.01, paired bootstrap, 95% CIs exclude zero) while keeping a
~60× faster query path. "Query latency" is retrieval against a built segment;
document embeddings are precomputed offline (as in any vector-DB deployment).
Full method, CIs, and honest caveats: [`SCORE_ANALYSIS.md`](SCORE_ANALYSIS.md).

**L5M is faster because:**
- Memory-mapped binary segments (no JSON parsing)
- Hard gates filter before expensive semantic scoring
- Deterministic hashing (no model inference on hot path)
- Pure Rust, zero network calls

**L5M is more accurate because:**
- Hybrid semantic + lexical matching
- 5D memory model captures temporal, contextual, and trust dimensions
- Parent aggregation reduces duplicates

**L5M is more secure because:**
- Tenant, policy, trust, temporal, and context gates execute *before* scoring
- Proof-bearing output includes source hashes and trust levels
- Memory content treated as data, never as instructions

[Quick Start](#quick-start) • [Benchmarks](#benchmarks) • [Architecture](#architecture) • [Docs](https://docs.rs/l5m-core) • [Examples](#examples)

---

## Quick Start

Get from zero to your first query in under 5 minutes.

### Give your AI agent memory (MCP — fastest path)

L5M ships an [MCP server](crates/l5m-mcp/) that plugs into Claude Desktop,
Claude Code, ChatGPT, or any MCP host — durable `remember`/`recall`/`forget`
with the security principal bound by the host, not the agent:

```bash
cargo install --path crates/l5m-mcp
claude mcp add l5m-memory -e L5M_DATA_DIR=~/.l5m -- l5m-mcp
```

See [crates/l5m-mcp/README.md](crates/l5m-mcp/README.md) for Claude Desktop
config and the multi-agent isolation pattern.

### Install

```bash
cargo install l5m-cli
```

Or download pre-built binaries from [Releases](https://github.com/jbrorepo/l5m/releases).

### Compile Example Memory

```bash
# Download example data
curl -O https://raw.githubusercontent.com/jbrorepo/l5m/main/examples/seed_memories.json

# Compile into binary segment
l5m-cli compile --input seed_memories.json --output demo.segment --epoch 1
```

### Query

```bash
l5m-cli query \
  --segment demo.segment \
  --tenant 1 \
  --query "How long do we retain production database backups?" \
  --as-of 1770000000 \
  --context-mask 0xffff \
  --policy-mask 0xffff \
  --trust-floor 4 \
  --max-capsules 8
```

**Output:** Proof-bearing `MemoryFrame` with claims, evidence, trust levels, validity windows, source hashes, and coverage statistics.

See [docs/QUICKSTART.md](docs/QUICKSTART.md) for a complete walkthrough.

---

## 5D Memory Model

L5M organizes memory across five dimensions:

1. **Semantic**: Anchors, entities, 256-bit fingerprint, int8 residual vector
2. **Temporal**: Validity windows, observation time, verification time, supersession
3. **Context**: Tenant, environment, project, user group, sensitivity, task type
4. **Relation**: Supports, contradicts, supersedes, depends on, derived from
5. **Veracity**: Trust level, source ID, policy mask, content hash, poison risk

### Gates Before Scoring

Unlike traditional retrieval systems, L5M applies authorization and policy gates *before* semantic scoring:

```
All Capsules
    ↓
[Tenant Gate] ──────→ Unauthorized capsules excluded
    ↓
[Context Gate] ─────→ Wrong context excluded
    ↓
[Policy Gate] ──────→ Insufficient permissions excluded
    ↓
[Temporal Gate] ────→ Expired/future capsules excluded
    ↓
[Trust Gate] ───────→ Low-trust capsules excluded
    ↓
Candidate Set (safe to score)
    ↓
[Semantic Scoring] ─→ Rank by relevance
    ↓
Top-K Results
```

**Why this matters:** No unauthorized capsule can leak through semantic similarity. Security is enforced before performance optimization.

**See it for yourself** — a runnable proof that a *perfect-match* secret is never
scored for an unauthorized caller (cross-tenant, and missing-clearance), yet is
returned to the authorized one. The process exits non-zero if any gate leaks:

```bash
cargo run -p l5m-core --example leak_demo
```

```
[PASS] Cross-tenant isolation   — tenant 42 queries the exact secret text
[PASS] Policy clearance gate    — tenant 7 caller WITHOUT the 0x4 clearance bit
[PASS] Authorized retrieval     — tenant 7 caller WITH the 0x4 clearance bit
RESULT: all gates held.
```

---

## Benchmarks

All numbers are reproducible (commands in [`OPTIMIZATION_REPORT.md`](OPTIMIZATION_REPORT.md))
and significance-tested ([`SCORE_ANALYSIS.md`](SCORE_ANALYSIS.md)).

### Accuracy — LongMemEval held-out (n=450), top-k 10

| Metric | L5M native hybrid | Chroma + MiniLM (vector DB) | Δ vs vector DB | significant? |
|--------|------------------:|----------------------------:|---------------:|--------------|
| Recall@5 | **0.899** | 0.861 | +0.039 | yes (p≈0.001) |
| Recall@10 | **0.954** | 0.926 | +0.028 | yes (p≈0.010) |
| MRR | **0.867** | 0.832 | +0.036 | yes (p≈0.003) |
| Recall@1 | 0.505 | 0.474 | +0.031 | — |

Paired bootstrap (10k resamples), 95% CIs exclude zero on R@5/R@10/MRR → L5M's
hybrid is **statistically more accurate than a real, widely-deployed vector DB**.
Against L5M's *own* strong lexical baseline the dense signal adds a smaller,
significant R@10 gain (+0.016); honest per-metric detail in `SCORE_ANALYSIS.md`.

### Latency

| | L5M (query hot path) | Chroma + MiniLM |
|--|---------------------:|----------------:|
| p50 retrieval (dev-50) | **~1.8 ms** | ~110 ms |

Retrieval against a built segment; document embeddings are precomputed offline,
the same assumption a vector-DB deployment makes.

### Scale & multi-tenant (synthetic)

| Corpus | Query p50 |
|--------|----------:|
| 1M capsules, single tenant | 50.7 ms |
| 1M capsules, **1000 tenants** (query one) | **0.88 ms** |

Tenant isolation — a *security* boundary — is also the latency win: a query only
touches its own tenant's slice (~58× here). The LSH semantic index is proven to
return the same top-1 as an exact scan (`ann_semantic` test).

> Note: a prior `AUDIT_REPORT.md` (written by an automated tool) over-stated the
> results. It is retained only as a record; trust `VALIDATION_REPORT.md` and
> `SCORE_ANALYSIS.md`, which are reproducible from this repo.

---

## Architecture

### Immutable Memory-Mapped Segments

L5M compiles JSON memory capsules into immutable binary segment files:

```
┌─────────────────────────────────────┐
│ Segment File (memory-mapped)        │
├─────────────────────────────────────┤
│ Header (magic, version, offsets)    │
│ Metadata Records (fixed-size)       │
│ String Area (claims, evidence)      │
│ Relation Area (graph edges)         │
│ Index Summary (counts, hashes)      │
└─────────────────────────────────────┘
         ↓
    mmap() - no JSON parsing
         ↓
    Build indexes in memory
         ↓
    Ready for queries (<10ms)
```

**Benefits:**
- No JSON deserialization on hot path
- OS page cache optimization
- Atomic deployment (swap file)
- Validation via checksums

### Semantic Fingerprinting

L5M uses deterministic hashing instead of learned embeddings:

1. Extract terms (3+ chars, normalized)
2. Generate n-gram features (1-gram, 2-gram, 3-gram)
3. Hash features with Blake3
4. Set bits in 256-bit fingerprint
5. Accumulate int8 residual vector (64 elements)

**Scoring combines:**
- Entity overlap (4.0x weight)
- Anchor overlap (1.2x weight)
- Hamming distance on fingerprint (2.0x weight)
- Dot product on residual (1.0x weight)
- Trust level, freshness, context specificity
- Relation support bonus, poison penalty

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for detailed diagrams and explanations.

---

## Examples

### Conversational Memory

```rust
use l5m_core::{Segment, MemoryProbe, retrieve};

// Load conversation history segment
let segment = Segment::open("conversations.segment")?;

// Query with temporal context
let probe = MemoryProbe::build(
    "What did the user say about their preferences?",
    tenant_id,
    current_timestamp,
    context_mask,
    policy_mask,
    trust_floor
);

let frame = retrieve(&segment, &probe)?;

for capsule in frame.capsules {
    println!("Claim: {}", capsule.claim);
    println!("Evidence: {}", capsule.evidence);
    println!("Trust: {}/10", capsule.trust_level);
    println!("Valid: {} to {:?}", capsule.valid_from, capsule.valid_until);
    println!("Source: {}", hex::encode(capsule.source_hash));
    println!();
}
```

See [examples/](examples/) for complete runnable examples:
- `conversational-memory/` - Chat bot with memory
- `document-retrieval/` - RAG-style Q&A
- `multi-tenant/` - Isolated multi-user system

---

## Use Cases

**L5M is ideal for:**
- ✅ Conversational AI with long-term memory
- ✅ RAG systems requiring fast retrieval
- ✅ Multi-tenant applications needing isolation
- ✅ Systems with compliance requirements (audit trails)
- ✅ Edge deployment (no GPU, no network)
- ✅ High-throughput query workloads

**L5M may not be ideal for:**
- ❌ Pure semantic search with no security/tenancy requirements (a plain vector DB is simpler)
- ❌ Multi-node distributed deployments (single-node today; immutable segments make replication straightforward — on the roadmap)

---

## Production Features

- **Real-time writes**: LSM delta layer — amortized O(1) inserts, WAL durability
  (acknowledged writes survive crashes), automatic compaction
- **Time-travel recall**: bi-temporal capsules (`valid_from`/`valid_until`/`observed_at`)
  with `as_of` point-in-time queries and supersession — ask "what did we believe then?"
- **Encryption at rest**: ChaCha20-Poly1305 sealed segments with pluggable key providers
- **Tamper-evident audit**: hash-chained access log with `verify` endpoint
- **AuthN**: JWT (HS256/RS256) or API key; principal resolved from verified
  credentials, never from the request body
- **Abuse resistance**: per-tenant token-bucket rate limiting, request body caps
- **Observability**: Prometheus metrics, structured JSON logs, health/readiness probes
- **MCP server**: [`l5m-mcp`](crates/l5m-mcp/) — security-gated memory for Claude,
  ChatGPT, Copilot, and any MCP host, with the principal bound at startup
- **Python SDK**: [`clients/python`](clients/python/) — dependency-free client
- **Supply chain**: signed releases (cosign/Sigstore) + CycloneDX/SPDX SBOM,
  `cargo-deny`, continuous fuzzing

See [docs/segment-format.md](docs/segment-format.md) for the on-disk format and
[docs/security-model.md](docs/security-model.md) for the security model.

---

## Installation

### From crates.io

```bash
cargo install l5m-cli
```

### From source

```bash
git clone https://github.com/jbrorepo/l5m.git
cd l5m
cargo build --release
```

### Pre-built binaries

Download from [Releases](https://github.com/jbrorepo/l5m/releases):
- Linux x86_64
- macOS x86_64 / ARM64
- Windows x86_64

### Docker

```bash
docker pull jbrorepo/l5m:latest
docker run -v $(pwd)/data:/data jbrorepo/l5m query --segment /data/demo.segment --query "test"
```

---

## Documentation

**Security**
- [Threat Model](THREAT_MODEL.md) — assets, trust boundaries, threats → mitigations → tests
- [Security Policy](SECURITY.md) — vulnerability disclosure + assurance in CI

**Evidence (reproducible)**
- [Score Analysis](SCORE_ANALYSIS.md) — accuracy/latency with CIs + significance
- [Optimization Report](OPTIMIZATION_REPORT.md) — what changed, with reproduce commands
- [Validation Report](VALIDATION_REPORT.md) — independent re-derivation of every claim

**Guides**
- [Quick Start](docs/QUICKSTART.md) — first query in minutes
- [Architecture](docs/architecture.md) — how L5M works internally
- [Vector-DB peer benchmark](bench/README.md) — how the comparison is run

---

## Roadmap

**Shipped:**
- [x] Real-time writes (LSM delta, WAL durability, automatic compaction)
- [x] Optional learned embeddings (hybrid lexical ⊕ dense with RRF)
- [x] Gate-filtered dense ANN
- [x] Encryption at rest (sealed segments)
- [x] Tamper-evident audit log
- [x] HTTP server with JWT auth, rate limiting, Prometheus metrics
- [x] Python SDK (dependency-free)
- [x] MCP server (`l5m-mcp`) — memory for Claude/ChatGPT/Copilot agents
- [x] Signed releases + SBOM (cosign/Sigstore)
- [x] Continuous fuzzing (in-tree + cargo-fuzz in CI)
- [x] OpenAPI spec + TypeScript SDK (zero runtime deps)
- [x] Scoped API keys (read/write/admin, tenant-bindable) + JWKS key rotation
- [x] Per-tenant usage metering (Prometheus labels + admin `/v1/usage`)

**Next:**
- [ ] Memory-extraction pipeline (transcript → capsules, offline)
- [ ] Admin/ops API (checkpoint, compaction, tenant stats, audit export)

**Later:**
- [ ] Read replicas via immutable-segment shipping (HA)
- [ ] SIMD-optimized scoring
- [ ] Advanced policy expression language
- [ ] Multi-hop relation traversal

Community input welcome!

---

## Contributing

We welcome contributions! See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

**Ways to contribute:**
- 🐛 Report bugs via [GitHub Issues](https://github.com/jbrorepo/l5m/issues)
- 💡 Suggest features via [GitHub Discussions](https://github.com/jbrorepo/l5m/discussions)
- 📖 Improve documentation
- 🧪 Add test coverage
- ⚡ Optimize performance
- 🌍 Add language bindings

**Development:**

```bash
# Run tests
cargo test --workspace

# Run benchmarks
cargo run -p l5m-benchmarks -- longmemeval --input data/longmemeval_s_cleaned.json --mode hybrid-parent --top-k 10

# Format and lint
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
```

---

## Community

- **GitHub Discussions**: [Ask questions, share ideas](https://github.com/jbrorepo/l5m/discussions)
- **GitHub Issues**: [Report bugs, request features](https://github.com/jbrorepo/l5m/issues)
- **Discord**: [Join the community](https://discord.gg/yourinvite) (coming soon)

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

---

## Citation

If you use L5M in your research, please cite:

```bibtex
@software{l5m2026,
  title = {L5M: Low-Latency 5D Memory for AI},
  author = {jbrorepo},
  year = {2026},
  url = {https://github.com/jbrorepo/l5m}
}
```

---

## Acknowledgments

- Benchmark datasets: LongMemEval, ConvoMem, LoCoMo
- Inspired by MemPalace and modern retrieval systems
- Built with Rust and minimal dependencies

---

**Star ⭐ this repo if L5M helps your project!**

[Get Started](docs/QUICKSTART.md) • [Read the Docs](https://docs.rs/l5m-core) • [Join Discussion](https://github.com/jbrorepo/l5m/discussions)
