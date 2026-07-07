# Launch posts (drafts) — ready to fire

Internal drafts for getting external eyes on L5M. Post from a real account;
be present in the comments (that's where the value is). Lead with the *idea*
and the *honest benchmark*, not a feature list. Don't oversell — the crowds
below (HN, r/rust) punish hype and reward candor, which is our strength.

**Pre-flight checklist before posting:**
- [ ] CI is green on `main` (the badge is the first thing people click).
- [ ] `./scripts/demo.sh` runs clean on a fresh checkout.
- [ ] `reports/GATED_RETRIEVAL.md` reproduces (`cargo build --release -p l5m-bench` + the two commands).
- [ ] You can personally explain gate-before-scoring, the LSM delta, and the benchmark methodology in the comments.
- [ ] Optional: publish to crates.io first so `docs.rs` renders (adds a badge + discoverability).

---

## Show HN

**Title:** Show HN: L5M – a memory engine for AI agents that gates authorization before scoring

**Body:**

L5M is a multi-tenant memory/retrieval engine in Rust. The idea I wanted to
test: put authorization *inside* the engine, before relevance scoring, instead
of bolting a metadata filter onto a vector DB at the app layer.

Why it matters: a `where={"tenant": t}` filter can be forgotten on one code
path, and it can't express trust levels, validity windows, or clearance
policies anyway. So I built the gates (tenant / context / policy / trust /
temporal) into the retrieval path — they construct the authorized candidate
set *before* anything is scored, so an unauthorized memory is never even a
candidate.

I benchmarked it honestly against a real vector DB (Chroma + MiniLM), and kept
the results where I lose:

- Gated benchmark (100K memories, 100 tenants): L5M disclosed 0 of 26
  policy-embargoed memories; the perfectly-filtered vector DB disclosed 24/26
  (92%), because trust/temporal/policy don't exist in its model. Query p50
  0.28 ms vs ~130 ms.
- Accuracy on LongMemEval (n=450, paired bootstrap): native hybrid retrieval
  beats the vector DB on Recall@5/@10/MRR at p ≤ 0.01. But on synthetic
  paraphrase, learned embeddings out-rank my deterministic fingerprints
  (0.82 vs 0.51) — reported in the same table.

The isolation guarantee is checked by a `proptest` invariant over randomized
multi-tenant corpora, plus adversarial "perfect-match secret in another
tenant" tests, plus a coverage-guided fuzzer (which found and I fixed a real
159 GB unbounded-allocation DoS in the parser). `forbid(unsafe_code)`
everywhere except one documented mmap.

It ships with an MCP server (memory for Claude/ChatGPT/agents), Python + TS
SDKs, WAL durability, and a compliance-control map where each control links to
a test.

Honest limitations: single-writer (HA is designed, not built); the
deterministic fingerprint is weak on paraphrase without the hybrid path.

Repo + reproducible benchmarks: <REPO_URL>
Start here if you want to audit it: <REPO_URL>/blob/main/REVIEW.md

Happy to go deep on the gate-before-scoring design or the benchmark
methodology in the comments.

---

## r/rust

**Title:** L5M: a security-gated memory engine (Rust, forbid(unsafe), machine-checked isolation invariant)

**Body:**

I've been building a multi-tenant memory engine for AI agents and wanted to
share it with this crowd specifically for the Rust/systems angle.

The core idea is "gate before score": authorization gates run on the candidate
set before any relevance scoring, so an unauthorized record is never even
scored. The interesting Rust bits:

- **Machine-checked security invariant** via `proptest`: randomized
  multi-tenant corpora + probes, asserting no returned record violates any
  gate. It's a CI gate.
- **Untrusted-input parser** (`Segment::from_untrusted_bytes`) with a
  coverage-guided `cargo-fuzz` target *and* an in-tree mutational fuzzer. The
  fuzzer caught a real unbounded-allocation DoS pre-release.
- **`forbid(unsafe_code)`** on every crate but one; the single `unsafe` is a
  documented read-only mmap with a SAFETY comment.
- **LSM-style write layer**: bounded in-RAM buffer → sealed immutable runs →
  auto-compaction, taking live writes from O(N²) to amortized O(1), with an
  fsync'd WAL for crash durability.
- Zero-runtime-dependency SDKs, minimal dependency tree by policy (cargo-deny
  in CI, supply-chain-signed releases with SBOMs).

~14.5K LOC, 6-crate workspace, 3-OS CI. I tried hard to benchmark honestly
against a production vector DB and left in the columns where I lose.

Would genuinely value review of the retrieval hot path (`retrieve.rs`) and the
LSM delta (`store.rs`). Reviewer's guide: <REPO_URL>/blob/main/REVIEW.md

<REPO_URL>

---

## This Week in Rust (submit as a PR / issue to the newsletter repo)

**Section:** Project/Tooling Updates (or Crate of the Week nomination)

L5M — a security-gated memory engine for AI agents. Authorization gates
(tenant/policy/trust/temporal) are enforced before relevance scoring, verified
by a `proptest` invariant and continuous fuzzing; `forbid(unsafe_code)` except
one documented mmap. Includes an MCP server, honest significance-tested
benchmarks vs a production vector DB, and signed/SBOM'd releases.
<REPO_URL>

---

## lobste.rs

Tags: `rust`, `security`, `databases`. Use the Show HN body, trimmed. lobste.rs
skews senior and reads the code — make sure REVIEW.md and CI are pristine
first.

---

## Targeted expert outreach (higher signal than broadcast)

- **The Mem0 / Zep maintainers**, respectfully: "built an honest head-to-head
  harness against your OSS, here's the methodology and the gated-retrieval
  angle you don't currently cover — curious for your read." (Finish the QA
  head-to-head first so the comparison is complete and fair.)
- **RAG / agent-memory researchers** on the LongMemEval angle.
- **A paid senior Rust review** (e.g. via a well-known reviewer) if you want an
  independent named audit to cite — cheap relative to the credibility.
