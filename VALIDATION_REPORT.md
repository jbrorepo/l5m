# L5M Independent Validation Report

**Validator:** Claude (Opus 4.8), adversarial re-verification
**Date:** 2026-06-06
**Method:** Read the source, rebuilt from scratch, re-ran benchmarks from the raw datasets, and wrote new adversarial tests. Nothing here is taken from the existing `AUDIT_REPORT.md` (which was written by another AI agent and is itself one of the things under review).

---

## TL;DR

| Claim | Verdict | Proof |
|-------|---------|-------|
| Security gates enforce *before* scoring | ✅ **TRUE & ROBUST** | 7 new adversarial tests; perfect-match secrets blocked across all gate types |
| "53 tests pass, builds clean" | ✅ TRUE | reproduced: 53 passed, exit 0 |
| "3.4× faster than BM25" | ❌ **FALSE / INVERTED** | honest per-query cost: L5M is **4–8× slower**; the win comes from excluding L5M's build time while counting BM25's |
| LongMemEval accuracy | ⚠️ **It *is* BM25** | hybrid-parent = BM25 + 0.05 tiebreaker; recall identical to 4 decimals |
| "40% better recall" (ConvoMem) | ⚠️ **REAL NUMBER, WRONG ATTRIBUTION** | gain is from parent aggregation (generic) + hand-tuned heuristics, not the "5D memory" |
| "Exceeds market leaders" (MemPalace) | ❌ **UNSUBSTANTIATED** | no MemPalace implementation, run file, or source exists anywhere in the repo |
| Benchmark harness is leak-free | ⚠️ **NO** | retrieval path reads `ground_truth_ids.is_empty()` to decide abstention |

The engineering is real and the security model is genuinely good. But the **performance and accuracy marketing is not supported by the evidence**, and in the case of latency it is the opposite of true.

---

## 1. What holds up: the security gating ✅

This is the real differentiator and it is solid.

`retrieve_with_config` ([retrieve.rs:36-55](crates/l5m-core/src/retrieve.rs)) applies tenant / context / policy / temporal / trust gates to build the candidate set **before** any semantic scoring runs. I confirmed the existing tests are honest and then wrote a **stronger** adversarial suite where each unauthorized capsule is a *byte-identical perfect match* for the query — i.e. it would rank #1 if the gates didn't run first.

**New test file:** [crates/l5m-core/tests/adversarial_gates.rs](crates/l5m-core/tests/adversarial_gates.rs)

```
running 7 tests
test tenant_gate_blocks_perfect_match_secret ............ ok
test policy_gate_blocks_perfect_match_secret ............ ok
test trust_gate_blocks_perfect_match_secret ............ ok
test temporal_gate_blocks_expired_perfect_match_secret . ok
test temporal_gate_blocks_future_perfect_match_secret .. ok
test context_gate_blocks_perfect_match_secret .......... ok
test no_returned_capsule_violates_any_hard_gate ........ ok
test result: ok. 7 passed; 0 failed
```

Each test also asserts a *positive control* (an authorized identical-text capsule **is** returned), proving the secret was blocked by the gate, not by a failure to match. **The "no unauthorized capsule leaks through semantic similarity" claim is true.**

---

## 2. The latency claim is inverted ❌

**Claim (README):** L5M 27 ms vs BM25 91 ms on LongMemEval → "3.4× faster".

**Root cause:** the harness times the two systems asymmetrically.

- **L5M** ([l5m_session_verbatim.rs:70-89](crates/l5m-benchmarks/src/modes/l5m_session_verbatim.rs)): the reported `total_retrieval_ns` covers **only** the `l5m_retrieve()` call against an already-compiled, mmap'd segment. The segment *compile* — writing JSON, building the binary segment, mmapping — is recorded separately as `build_or_load_ns` and **excluded** from the headline.
- **BM25** ([mod.rs:168-192](crates/l5m-benchmarks/src/modes/mod.rs) + [bm25.rs:32-93](crates/l5m-benchmarks/src/modes/bm25.rs)): `total_retrieval_ns` wraps the **entire** call, which rebuilds the document-frequency index from scratch **every query** and then scores. `build_or_load_ns = 0`.

The report's headline percentile uses `total_retrieval_ns` for both ([report.rs:371](crates/l5m-benchmarks/src/report.rs)).

**Why this matters here specifically:** in these benchmarks *every query has its own document set*, so L5M compiles a **fresh segment per query** — the build is not amortized across queries. The harness's own unit test asserts `build_or_load_ns > total_retrieval_ns` ([mod.rs:609](crates/l5m-benchmarks/src/modes/mod.rs)). So the excluded cost is the *dominant* cost.

**Measured, from the committed run files that back the README (450 held-out queries):**

| | Reported P50 | Excluded build P50 | **Honest P50 (build+retrieval)** |
|---|---|---|---|
| L5M hybrid-parent | 25.8 ms | **323.9 ms** | **350.5 ms** |
| BM25 | 87.4 ms | 0 | 87.4 ms |

→ Counting the build for both, **L5M is 4.0× *slower***, not 3.4× faster.

**Reproduced from raw data (fresh run I executed, dev-50 split):**

| | Reported P50 | Excluded build P50 | Honest P50 |
|---|---|---|---|
| L5M hybrid-parent | 1.45 ms | 65.5 ms | 67.2 ms |
| BM25 | 12.5 ms | 0 | 12.5 ms |

→ **5.4× slower.** ConvoMem shows the same: honest L5M 15.7 ms vs BM25 1.9 ms (~8× slower), versus the claimed "1.4× faster".

A fair latency comparison must either (a) include build for both, or (b) give BM25 a pre-built inverted index and time scoring only. Under either, L5M as benchmarked does not win. *(L5M's mmap design genuinely could win in a "build once, query many over one fixed corpus" deployment — but that scenario is neither what these benchmarks measure nor what the numbers in the README represent.)*

---

## 3. The accuracy is BM25's accuracy ⚠️

On LongMemEval the README lists every accuracy metric as "Parity". That is literally correct — and the reason is that **"hybrid-parent" ranking is BM25**.

[mod.rs:231](crates/l5m-benchmarks/src/modes/mod.rs): `hybrid_score = bm25_score + l5m_rank * 0.05`. The L5M / "5D semantic memory" signal is a 0.05-weight tiebreaker on top of raw BM25 scores. Their own frozen config confirms it: `bm25_weight: 1.0, l5m_rank_weight: 0.05` ([configs/benchmark/longmemeval.json](configs/benchmark/longmemeval.json)).

**Measured (450 held-out queries, my analysis of the committed run files):**

| Metric | L5M hybrid-parent | BM25 |
|---|---|---|
| Recall@1 | 0.5244 | 0.5349 |
| Recall@5 | 0.8789 | 0.8772 |
| Recall@10 | 0.9387 | 0.9387 |
| NDCG@10 | 0.8625 | 0.8660 |
| MRR | 0.8746 | 0.8819 |

Rank-1 result identical on **91%** of queries (100% on the fresh dev-50 run). The "5D memory model" contributes **nothing measurable** to ranking accuracy on LongMemEval — BM25 is marginally *higher* on R@1, MRR, and NDCG.

---

## 4. The "+40% recall" on ConvoMem is real, but misattributed ⚠️

I reproduced it exactly: overall R@5 **0.7490 (hybrid-parent) vs 0.5343 (BM25)** across all 75,336 queries. But where does it come from?

**Per-category R@5 (full ConvoMem):**

| Category | hybrid-parent | BM25 | n |
|---|---|---|---|
| abstention-evidence | 0.8696 | **0.8696** | 14,910 |
| assistant-facts-evidence | 0.7969 | 0.6991 | 12,745 |
| changing-evidence | 0.6768 | 0.3579 | 18,323 |
| implicit-connection-evidence | 0.6996 | 0.2025 | 7,546 |
| preference-evidence | 0.8031 | 0.2999 | 5,079 |
| user-evidence | 0.6902 | 0.5241 | 16,733 |

The entire delta is in the non-abstention categories. The **only** algorithmic difference between `hybrid-parent` and `bm25` mode there is **parent aggregation** ([mod.rs:283-339](crates/l5m-benchmarks/src/modes/mod.rs)) — grouping child capsules by parent and ranking parents with a density bonus. That is a **generic post-processing technique that applies to any retriever**, including BM25. The honest comparison is "BM25 + parent aggregation" vs "L5M + parent aggregation", not raw-BM25 vs aggregated-L5M. The README attributes a structural/aggregation win to the "5D memory model."

It is also heavily hand-tuned to ConvoMem: an ~80-phrase keyword list and typed-answer validators ([mod.rs:372-488](crates/l5m-benchmarks/src/modes/mod.rs)) decide when to abstain. This is dataset-specific engineering, not a general memory capability.

**Decomposition (ConvoMem `changing-evidence`, n=2,000, top_k=10) — the decisive evidence:**

| Mode | R@5 | R@10 | What it isolates |
|---|---|---|---|
| BM25 raw lexical | 0.4645 | 0.6105 | baseline, no aggregation |
| L5M pure 5D semantic (`l5m-session-verbatim`) | **0.4960** | 0.6210 | the "5D memory" *alone* → only **+0.03** over BM25 |
| hybrid-parent (headline) | **0.8165** | 0.8165 | adds parent aggregation → **+0.32** |

The "5D semantic memory" by itself (0.496) is barely above raw BM25 (0.465) and nowhere near the headline 0.817. **Essentially the entire ConvoMem advantage is parent aggregation, not the memory model.**

**Corroborating speed observation:** the pure-L5M modes are also extremely slow — a 2,000-query subset takes >120 ms/query (segment-compile dominated) and one run timed out at 240 s, while `bm25`/`hybrid-parent` finish the same subset in seconds. Independent confirmation of §2.

---

## 5. Ground-truth leakage in the harness ⚠️

`apply_insufficient_evidence_policy` ([mod.rs:341-352](crates/l5m-benchmarks/src/modes/mod.rs)) decides to emit `"insufficient-evidence"` by reading **`item.ground_truth_ids.is_empty()`** — i.e. it inspects the answer label. For abstention items the score is then force-set to 1.0 across all metrics ([main.rs:287-307](crates/l5m-benchmarks/src/main.rs)), and `score_abstention` returns true on the `"insufficient-evidence"` marker ([convomem.rs:270-275](crates/l5m-benchmarks/src/adapters/convomem.rs)).

A real system at inference time cannot know whether the ground truth set is empty. This is a label leak. It is applied to **both** modes equally, so it does *not* explain the L5M-vs-BM25 gap — but it inflates the **absolute** numbers (abstention is ~20% of ConvoMem) and means the harness is not a clean simulation of inference-time behavior.

---

## 6. "Exceeds market leaders" has no evidence ❌

The headline comparison row — `MemPalace | ~0.920 | ~100ms | Vector DB + GPU` — exists **only** in the README/launch-plan marketing tables. There is **no MemPalace implementation, no run file, no adapter, and no source** anywhere in the repo (the only mentions are prose). Every competitor claim is an unbacked estimate (note the `~`). There is zero measured evidence in this repository of beating any real competing system. The only baseline actually implemented and run is BM25 — and against BM25, see sections 2–4.

---

## 7. Minor integrity issues

- README links to `docs/REPRODUCE.md`, `docs/SECURITY.md`, `docs/COMPARISON.md`, `docs/DEPLOYMENT.md` — **all 404.** The "verify our benchmarks" link (REPRODUCE.md) does not exist.
- `cargo install l5m-cli` and `docs.rs/l5m-core` are advertised but not published.
- The existing `AUDIT_REPORT.md` ("VERDICT: LEGITIMATE", by Kiro AI) did not catch the latency asymmetry, the 0.05-weight passthrough, the aggregation attribution, or the label leak. It should not be cited as independent verification.

---

## How to reproduce everything here

```bash
cargo build --release --workspace
cargo test -p l5m-core --release --test adversarial_gates     # security proof (7 tests)

# accuracy + honest latency (fresh, from raw data)
target/release/l5m-benchmarks longmemeval --input data/longmemeval_s_cleaned.json \
  --mode hybrid-parent --top-k 10 --split-file runs/lme_split_seed42.json --dev-only --out runs/_hp.jsonl
target/release/l5m-benchmarks longmemeval --input data/longmemeval_s_cleaned.json \
  --mode bm25 --top-k 10 --split-file runs/lme_split_seed42.json --dev-only --out runs/_bm.jsonl
python scripts/analyze_runs.py runs/_hp.jsonl "L5M" runs/_bm.jsonl "BM25"
```

`scripts/analyze_runs.py` (added by this validation) computes recall, the reported vs honest latency, and the ranking overlap that proves hybrid-parent ≈ BM25.

---

## Bottom line

To actually "exceed market leaders with a security-gated memory platform that is faster and more reliably accurate," three things have to change:

1. **Latency:** report a like-for-like number (build counted for both, or pre-built index for both). Today's "faster" is an artifact of excluding L5M's dominant cost. Lean into the real strength — *amortized* query latency over a fixed, pre-compiled corpus — and benchmark *that* honestly.
2. **Accuracy:** the 5D semantic signal currently rides at 0.05 weight and adds nothing. Either make it load-bearing (and prove it beats BM25 *with the same parent aggregation*) or stop attributing BM25's recall to it.
3. **Competitors:** implement and run at least one real market leader (MemPalace / a vector DB), or remove the comparison. And add a real benchmark against a memory product, not just BM25.

The **security gating is genuinely strong and differentiated** — that is the claim worth leading with, and it is the one fully backed by proof.
