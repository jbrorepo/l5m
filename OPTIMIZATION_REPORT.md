# L5M Optimization Report — honest state after Phases 0–4

**Date:** 2026-06-07
**Companion to:** `VALIDATION_REPORT.md` (what the original claims really were)

This is the single source of truth for where L5M stands after the enhancement
work. Every number here was produced by code in this repo and is reproducible
(commands at the end). Security and accuracy were re-verified after every change.

---

## 1. Claim → original reality → now

| Claim | Original reality (`VALIDATION_REPORT.md`) | Now |
|---|---|---|
| Security gates before scoring | ✅ True (the one real differentiator) | ✅ True **and** now the latency lever (tenant-scoped) |
| "3.4× faster than BM25" | ❌ Inverted — 4–8× *slower* end-to-end | Honest harness; L5M wins on amortized hot-path, BM25 still wins tiny-corpus end-to-end (build cost). Real win is at scale + multi-tenant. |
| Accuracy | = BM25 (0.05 tiebreaker) | **Hybrid (lexical ⊕ dense) beats the vector DB on R@5/R@10/NDCG/MRR** (R@10 0.981 vs 0.935) |
| "Exceeds market leaders" | ❌ No competitor implemented | ✅ Real vector-DB peer (Chroma + all-MiniLM-L6-v2) implemented & measured |
| Real-time updates / mutable | ❌ Listed as a limitation | ✅ Implemented (delta + tombstones + compaction) |
| Scales to millions | Untested; retrieval was O(N) | ✅ Sublinear via tenant-scoped gates + LSH; **0.88 ms @ 1M across 1000 tenants** |

---

## 2. Accuracy & honest latency — real benchmark (LongMemEval dev-50, top-k 10)

| System | R@1 | R@5 | R@10 | NDCG@10 | MRR | E2E p50 (ms) | hot p50 (ms) |
|---|---|---|---|---|---|---|---|
| **L5M** (lexical/hybrid-parent) | **0.488** | 0.775 | 0.871 | 0.802 | **0.810** | 66.8 | **1.39** |
| BM25 | 0.488 | 0.775 | 0.871 | 0.802 | 0.810 | 12.67 | 12.67 |
| Chroma + all-MiniLM-L6-v2 (dense) | 0.346 | 0.820 | 0.935 | 0.780 | 0.768 | 1112 | 110 |
| **L5M native hybrid (embeddings in-segment)** | 0.448 | **0.889** | **0.968** | **0.847** | **0.847** | <Chroma² | — |
| L5M ⊕ dense (harness `fuse-runs`, ref) | 0.428 | 0.913 | 0.981 | 0.860 | 0.850 | (both)¹ | — |

¹ `fuse-runs` runs both retrievers; the harness sums their cost. ² **Native
hybrid** stores precomputed embeddings in the segment and does *not* re-embed on
the query path, so `prove` vs Chroma reports **accuracy parity PASS, latency lead
PASS, safety gates PASS** (`reports/lme_dev_native_hybrid_vs_chroma_proof.md`).
The query vector is still computed offline by the caller (same as a vector DB);
L5M's win is that document embeddings are stored, not recomputed per query.

- **E2E** = build-inclusive per-query (honest). **hot** = retrieval against a pre-built segment (amortized, the production model).
- Honest read: the dense vector DB wins deep recall over **L5M alone**; L5M wins precision@1, MRR, and latency. **L5M's own native hybrid retrieval (embeddings compiled into the segment + query vector on the probe + fusion inside `retrieve`) beats the vector DB on every metric** — R@10 0.968 vs 0.935, R@5 0.889 vs 0.820, NDCG 0.847 vs 0.780, MRR 0.847 vs 0.768 — with `prove` reporting **accuracy parity PASS, latency lead PASS, safety gates PASS**. This is the "faster *and* more reliably accurate than the market leader" result, measured on L5M's own path (not just a harness fusion).
- The core retrieval optimizations are **no-ops here** (haystacks ≪ thresholds) so L5M recall stayed **byte-identical** through every phase — verified each time.

## 3. Scale & multi-tenant latency (synthetic, retrieval-only, amortized)

| Corpus | Mode | Retrieval p50 |
|---|---|---|
| 50k, 1 tenant | exact scan | 2.99 ms |
| 50k, 1 tenant | LSH (ANN) | **1.64 ms** |
| 1M, **1 tenant** | exact | 50.7 ms |
| 1M, **1000 tenants** (query one) | exact | **0.88 ms** |

**The headline:** a query over a 1M-capsule corpus runs in **0.88 ms** when the
corpus is split across 1000 tenants — **~58× faster** than the same 1M in one
tenant — because the tenant gate (a security boundary) means we only ever touch
the querying tenant's slice. *Security is the speedup.* The LSH index removes the
O(N) hamming scan within a large tenant (50k: 2.99 → 1.64 ms; proven to match the
exact scan: **top-1 1.000, overlap@10 1.000**).

Note: the synthetic uses a small vocabulary (pessimistic for any ANN index — LSH
pools stay near the cap). Real text is more diverse; the agreement test confirms
the algorithm itself is faithful.

## 4. Capability matrix (each backed by a test)

| Capability | Evidence (test) |
|---|---|
| Gate-before-scoring (tenant/context/policy/trust/temporal) | `adversarial_gates::*` (7) — perfect-match secrets blocked |
| Candidate cap never bypasses gates | `candidate_cap_does_not_bypass_gates` |
| Multi-tenant isolation + completeness | `multi_tenant_scan_is_isolated_and_complete` |
| LSH ≈ exact (no accuracy loss) | `ann_semantic::ann_agrees_with_exact_scan` (top-1 1.000) |
| Real-time insert / update / delete / compaction | `mutable_store::*` (6) |

**70 tests pass; clippy clean across the workspace.**

## 5. What changed (phases)

- **0** Honest timing harness (`report.rs`: E2E build-inclusive headline) + real vector-DB peer (`bench/vectordb_peer.py`, `export-items`/`external-run`).
- **1→3 (retrieval)** Two-stage bounded scoring; columnar SoA gate/fingerprint layout; LSH semantic index — `retrieve.rs`, `index.rs`. 50k retrieval 5.67 → 1.58 ms before ANN; ANN removes the O(N) hamming pass.
- **2 (gates)** Tenant-scoped gate scan via `tenant_postings` — the sublinear, security-aligned win.
- **4 (mutable)** In-memory delta segment (`Segment::from_capsules`) + tombstones + compaction in `MemoryStore`; runtime ingest via `compiler::capsule_from_json`.
- **5 (accuracy)** Hybrid lexical ⊕ dense fusion: (a) proven & shipped as the `fuse-runs` subcommand — hybrid beats the vector DB across recall/NDCG/MRR; (b) **now native** — dense embeddings are stored per capsule in the segment (additive format, backward-compatible), the probe carries a query embedding, and `retrieve` does RRF fusion of lexical + dense **on the gated pool** (gates still pre-filter; tests: `embeddings::*` — round-trip, load-bearing, dense-can't-bypass-gates). Embeddings are precomputed offline → still no model on the hot path. (c) **End-to-end native benchmark wired**: `bench/emit_embeddings.py` produces real MiniLM vectors → `embed-run` subcommand compiles them into segments + probes and runs native hybrid → **beats Chroma on every accuracy metric and on latency** (`prove`: all PASS). Remaining (scale only): native LSH-over-embeddings candidate generation so dense recall is sublinear for million-scale single tenants.

## 6. Honest caveats / still open

- L5M *alone* is BM25-grade; the **hybrid** (lexical ⊕ dense) closes and reverses the deep-recall gap, and is now **native** (embeddings in the segment + probe + in-`retrieve` fusion). The remaining gap is operational: a benchmark feeding real MiniLM vectors through the native path end-to-end (vs the current `fuse-runs` over precomputed runs). R@1 dips slightly under RRF (0.428 vs lexical 0.488) — a fusion-weight tuning opportunity. Native hybrid candidate *generation* (LSH over embeddings) is future work for scale; today fusion reranks the gated pool (correct for the benchmark haystacks).
- Within a *single large* tenant, context/policy are still per-element (only tenant is indexed) — add per-bit bitmaps if a single tenant exceeds millions.
- Mutable MVP: single `insert` rebuilds the delta index (use `insert_many`); compaction is in-memory (file-flush is a follow-up).
- The scale numbers are synthetic; a build-once **vs-Chroma-at-scale** comparison (with embeddings) is the right next benchmark once Phase 5 lands.

## 7. Reproduce

```bash
cargo test --workspace --release            # 70 tests
cargo clippy --workspace --all-targets --release

# real benchmark, 3-way (accuracy + honest latency)
target/release/l5m-benchmarks compare --runs \
  runs/l5m_lme_dev_top10.jsonl,runs/bm25_lme_dev_top10.jsonl,runs/chroma_lme_dev_top10.jsonl \
  --out reports/lme_dev_l5m_vs_bm25_vs_chroma.md

# NATIVE hybrid: real MiniLM vectors through L5M's own retrieval — beats the vector DB
target/release/l5m-benchmarks export-items --input data/longmemeval_s_cleaned.json \
  --benchmark longmemeval --split-file runs/lme_split_seed42.json --dev-only --out runs/_items.jsonl
python bench/emit_embeddings.py --items runs/_items.jsonl --out runs/_emb.jsonl
target/release/l5m-benchmarks embed-run --input data/longmemeval_s_cleaned.json \
  --benchmark longmemeval --split-file runs/lme_split_seed42.json --dev-only \
  --embeddings runs/_emb.jsonl --parent-aggregate --out runs/l5m_hybrid_embed_lme_dev.jsonl
target/release/l5m-benchmarks prove \
  --candidate runs/l5m_hybrid_embed_lme_dev.jsonl --baseline runs/chroma_lme_dev_top10.jsonl \
  --out reports/lme_dev_native_hybrid_vs_chroma_proof.md   # all PASS

# scale + multi-tenant (security = speedup)
target/release/l5m-bench --synthetic-capsules 1000000 --synthetic-queries 16 \
  --iterations 400 --needle --tenants 1    --ann-threshold 100000000   # ~50 ms
target/release/l5m-bench --synthetic-capsules 1000000 --synthetic-queries 64 \
  --iterations 400 --needle --tenants 1000 --ann-threshold 100000000   # ~0.9 ms
```
