#!/usr/bin/env python3
"""Emit real sentence embeddings (all-MiniLM-L6-v2 via onnxruntime) for the
queries and documents of an exported benchmark, so L5M can run *native* hybrid
retrieval (dense vectors stored in the segment + query vector on the probe).

Input  : items.jsonl from `l5m-benchmarks export-items`
Output : embeddings.jsonl, one line per query:
         {"query_id", "query_embedding":[...], "doc_embeddings":{capsule_id:[...]}}

These are the SAME embeddings the Chroma peer uses — the point is to feed them
through L5M's own retrieval path instead of a separate vector DB.
"""
import argparse
import json
import sys
import time

from chromadb.utils import embedding_functions as ef


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--items", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--limit", type=int, default=None)
    args = ap.parse_args()

    embed = ef.DefaultEmbeddingFunction()  # all-MiniLM-L6-v2, 384-dim, ONNX

    items = []
    with open(args.items, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if line:
                items.append(json.loads(line))
    if args.limit is not None:
        items = items[: args.limit]

    t0 = time.time()
    total = len(items)
    with open(args.out, "w", encoding="utf-8") as out:
        for i, item in enumerate(items):
            docs = item["documents"]
            ids = [d["capsule_id"] for d in docs]
            texts = [(d["text"] if d["text"].strip() else " ") for d in docs]
            doc_vecs = embed(texts) if texts else []
            q_vec = embed([item["question"]])[0]
            out.write(
                json.dumps(
                    {
                        "query_id": item["query_id"],
                        "query_embedding": [float(x) for x in q_vec],
                        "doc_embeddings": {
                            cid: [float(x) for x in vec]
                            for cid, vec in zip(ids, doc_vecs)
                        },
                    }
                )
                + "\n"
            )
            if (i + 1) % 25 == 0 or (i + 1) == total:
                rate = (i + 1) / max(time.time() - t0, 1e-9)
                print(f"  {i + 1}/{total} ({rate:.1f}/s)", file=sys.stderr, flush=True)

    print(f"wrote embeddings for {total} queries -> {args.out}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
