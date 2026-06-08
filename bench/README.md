# Vector-DB peer baseline

A real, developer-relevant vector database baseline for L5M — **ChromaDB** (compiled
hnswlib ANN engine, used widely in production RAG) with **all-MiniLM-L6-v2**
sentence embeddings (the de-facto default RAG embedding model, via onnxruntime).
Not a strawman: a genuine semantic retriever.

## Why it's honest

The vector DB only decides the **ranking**. Parent mapping, ground truth, the
insufficient-evidence policy, and every metric are computed by the Rust harness
(`external-run`), byte-for-byte identical to how L5M and BM25 are scored. There is
no separate Python scoring path that could be tuned to favor either side.

- **Build time** = embedding the documents + indexing them (ingest).
- **Query time** = embedding the question + ANN search.

Both are measured per the benchmark's per-query document set, mirroring how the
L5M harness compiles a segment per query, so the end-to-end comparison is fair.

## Run it (LongMemEval dev-50)

```bash
# 1. Export resolved items (query + documents) from the dataset
target/release/l5m-benchmarks export-items \
  --input data/longmemeval_s_cleaned.json --benchmark longmemeval \
  --split-file runs/lme_split_seed42.json --dev-only \
  --out runs/_items_lme_dev.jsonl

# 2. Vector-DB peer produces rankings + timings  (pip install chromadb)
python bench/vectordb_peer.py \
  --items runs/_items_lme_dev.jsonl \
  --out runs/_rank_chroma_lme_dev.jsonl --top-k 10

# 3. Score the ranking through the identical Rust harness
target/release/l5m-benchmarks external-run \
  --input data/longmemeval_s_cleaned.json --benchmark longmemeval \
  --split-file runs/lme_split_seed42.json --dev-only \
  --rankings runs/_rank_chroma_lme_dev.jsonl \
  --mode-label vector-db-chroma-minilm --top-k 10 \
  --out runs/chroma_lme_dev_top10.jsonl

# 4. Compare (all latency reported both end-to-end and amortized)
target/release/l5m-benchmarks compare \
  --runs runs/l5m_lme_dev_top10.jsonl,runs/bm25_lme_dev_top10.jsonl,runs/chroma_lme_dev_top10.jsonl \
  --out reports/lme_dev_l5m_vs_bm25_vs_chroma.md
python scripts/analyze_runs.py \
  runs/l5m_lme_dev_top10.jsonl "L5M" runs/chroma_lme_dev_top10.jsonl "Chroma+MiniLM"
```

## First result (LongMemEval dev-50, top-k 10)

| System | R@1 | R@5 | R@10 | MRR | E2E P50 | Hot P50 |
|---|---|---|---|---|---|---|
| L5M hybrid-parent | **0.488** | 0.775 | 0.871 | **0.810** | 67 ms | **1.4 ms** |
| BM25 | 0.488 | 0.775 | 0.871 | 0.810 | 13 ms | 13 ms |
| Chroma + all-MiniLM-L6-v2 | 0.346 | **0.820** | **0.935** | 0.768 | 1112 ms | 110 ms |

Honest takeaways:
- The vector DB **wins on deep recall** (R@5/R@10): learned embeddings find
  paraphrased evidence our BM25-driven ranking misses. **This is the gap to close**
  (motivates the learned-embeddings phase).
- L5M **wins on precision@1 and MRR**, and is far faster end-to-end — the vector
  pipeline's cost is dominated by embedding inference.
- These datasets do **not** exercise security gates (all docs share one
  tenant/policy/trust). The gate-before-scoring advantage needs the dedicated
  filtered-retrieval benchmark (a later phase).

## Adding Qdrant later

The harness is DB-agnostic: it consumes a `rankings.jsonl`. Swap the Chroma calls
in `vectordb_peer.py` for a Qdrant client (run `docker run -p 6333:6333 qdrant/qdrant`)
to add Qdrant — its payload filtering is the truest peer for the gate comparison.
