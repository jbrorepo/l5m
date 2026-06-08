#!/usr/bin/env python3
"""Real vector-DB peer baseline for L5M.

Stack: ChromaDB (compiled hnswlib ANN engine, the same engine used in countless
production RAG apps) + all-MiniLM-L6-v2 sentence embeddings (the de-facto default
RAG embedding model, run via onnxruntime). This is a genuine, developer-relevant
vector database, not a strawman.

Contract (keeps scoring honest):
  - Input  : items.jsonl from `l5m-benchmarks export-items`
             {benchmark, query_id, question, documents:[{capsule_id, text}]}
  - Output : rankings.jsonl
             {query_id, ranked_capsule_ids:[..], build_ns, query_ns}
  - The vector DB only decides the *ranking*. Parent mapping, ground truth,
    the insufficient-evidence policy, and all metrics are computed by the Rust
    harness (`l5m-benchmarks external-run`), identically to L5M and BM25.

Build time  = embedding the documents + indexing them (ingest).
Query time  = embedding the question + ANN search.
Both are what a real vector-DB RAG pipeline pays per the benchmark's per-query
document set, mirroring how the L5M harness builds a segment per query.
"""
import argparse
import json
import sys
import time

import chromadb
from chromadb.config import Settings
from chromadb.utils import embedding_functions as ef


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--items", required=True, help="items.jsonl from export-items")
    ap.add_argument("--out", required=True, help="rankings.jsonl output")
    ap.add_argument("--top-k", type=int, default=10)
    ap.add_argument("--limit", type=int, default=None, help="cap number of items (debug)")
    ap.add_argument("--space", default="cosine", choices=["cosine", "l2", "ip"])
    args = ap.parse_args()

    # Load the embedding model once (all-MiniLM-L6-v2, 384-dim, ONNX).
    embed = ef.DefaultEmbeddingFunction()
    client = chromadb.Client(Settings(anonymized_telemetry=False, is_persistent=False))

    items = []
    with open(args.items, "r", encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if line:
                items.append(json.loads(line))
    if args.limit is not None:
        items = items[: args.limit]

    total = len(items)
    t_start = time.time()
    with open(args.out, "w", encoding="utf-8") as out:
        for i, item in enumerate(items):
            qid = item["query_id"]
            docs = item["documents"]
            ids = [d["capsule_id"] for d in docs]
            texts = [(d["text"] if d["text"].strip() else " ") for d in docs]

            ranked: list[str] = []
            build_ns = 0
            query_ns = 0
            if ids:
                coll = client.create_collection(
                    name=f"item_{i}",
                    embedding_function=embed,
                    metadata={"hnsw:space": args.space},
                )
                # BUILD: embed + index the documents (ingest cost).
                t0 = time.perf_counter_ns()
                coll.add(ids=ids, documents=texts)
                build_ns = time.perf_counter_ns() - t0

                # QUERY: embed the question + ANN search.
                k = min(args.top_k, len(ids))
                t1 = time.perf_counter_ns()
                res = coll.query(query_texts=[item["question"]], n_results=k)
                query_ns = time.perf_counter_ns() - t1
                ranked = res["ids"][0] if res.get("ids") else []

                client.delete_collection(name=f"item_{i}")

            out.write(
                json.dumps(
                    {
                        "query_id": qid,
                        "ranked_capsule_ids": ranked,
                        "build_ns": build_ns,
                        "query_ns": query_ns,
                    }
                )
                + "\n"
            )
            if (i + 1) % 25 == 0 or (i + 1) == total:
                rate = (i + 1) / max(time.time() - t_start, 1e-9)
                print(
                    f"  {i + 1}/{total} items ({rate:.1f}/s)",
                    file=sys.stderr,
                    flush=True,
                )

    print(f"wrote rankings for {total} items -> {args.out}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
