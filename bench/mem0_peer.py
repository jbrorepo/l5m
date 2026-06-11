#!/usr/bin/env python3
"""Mem0 OSS peer baseline for L5M — same contract as bench/vectordb_peer.py.

Stack (all local except the extraction LLM):
  - mem0ai OSS `Memory` — the real product pipeline: LLM fact extraction on
    add(), embedding + vector search (+ mem0's own update/dedup logic).
  - LLM: Anthropic claude-haiku-4-5 (comparable tier to mem0's documented
    gpt-4o-mini-class defaults). Requires ANTHROPIC_API_KEY in the
    environment — never hardcoded, never written to disk.
  - Embedder: fastembed all-MiniLM-L6-v2 (ONNX) — the same embedding model the
    Chroma peer uses, so the embedding quality is held constant across peers.
  - Vector store: Chroma (local, per-item collection).

Contract (identical to the vector-DB peer — scoring stays in the Rust harness):
  Input : items.jsonl from `l5m-benchmarks export-items`
  Output: rankings.jsonl {query_id, ranked_capsule_ids, build_ns, query_ns}

How ranking is derived: each document (one conversation session) is add()ed
with metadata {"capsule_id": ...}. mem0 extracts memories from it; at search
time the returned memories carry that metadata, and we rank capsule_ids by
first appearance in mem0's result order. If mem0 extracted nothing from the
answer session — or its search doesn't surface those memories — the answer
capsule is simply absent, exactly as a real mem0-backed app would experience.

Build time = mem0.add() over all documents (LLM extraction + embed + index).
Query time = mem0.search() (embed + vector search + mem0 rerank).

Resumable: re-running skips query_ids already present in --out.
"""

import argparse
import json
import os
import sys
import tempfile
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed

EMBED_MODEL = "sentence-transformers/all-MiniLM-L6-v2"
LLM_MODEL = "claude-haiku-4-5"


def build_memory(workdir: str, worker: int):
    from mem0 import Memory

    config = {
        "llm": {
            "provider": "anthropic",
            "config": {"model": LLM_MODEL, "temperature": 0.0, "max_tokens": 2000},
        },
        "embedder": {
            "provider": "fastembed",
            "config": {"model": EMBED_MODEL},
        },
        "vector_store": {
            "provider": "chroma",
            "config": {
                "collection_name": f"mem0_peer_{worker}",
                "path": os.path.join(workdir, f"chroma_{worker}"),
            },
        },
        "history_db_path": os.path.join(workdir, f"history_{worker}.db"),
    }
    return Memory.from_config(config)


def process_item(item, top_k: int, workdir: str, worker: int):
    qid = item["query_id"]
    docs = item["documents"]
    memory = build_memory(workdir, worker)

    # BUILD: the real mem0 ingestion pipeline (LLM extraction per session).
    build_ns = 0
    extracted = 0
    for doc in docs:
        text = doc["text"].strip()
        if not text:
            continue
        t0 = time.perf_counter_ns()
        try:
            res = memory.add(
                [{"role": "user", "content": text[:30000]}],
                user_id=qid,
                metadata={"capsule_id": doc["capsule_id"]},
            )
            extracted += len(res.get("results", []) or [])
        except Exception as e:  # one bad session must not sink the item
            print(f"  [{qid}] add error: {type(e).__name__}: {e}", file=sys.stderr)
        build_ns += time.perf_counter_ns() - t0

    # QUERY: mem0 search (embed + ANN + mem0's reranking).
    t1 = time.perf_counter_ns()
    ranked = []
    try:
        res = memory.search(item["question"], user_id=qid, limit=max(top_k * 5, 25))
        seen = set()
        for hit in res.get("results", []) or []:
            cid = (hit.get("metadata") or {}).get("capsule_id")
            if cid is not None and cid not in seen:
                seen.add(cid)
                ranked.append(str(cid))
            if len(ranked) >= top_k:
                break
    except Exception as e:
        print(f"  [{qid}] search error: {type(e).__name__}: {e}", file=sys.stderr)
    query_ns = time.perf_counter_ns() - t1

    return {
        "query_id": qid,
        "ranked_capsule_ids": ranked,
        "build_ns": build_ns,
        "query_ns": query_ns,
        "_memories_extracted": extracted,
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--items", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--top-k", type=int, default=10)
    ap.add_argument("--limit", type=int, default=None, help="cap number of items")
    ap.add_argument("--workers", type=int, default=4)
    args = ap.parse_args()

    if not os.environ.get("ANTHROPIC_API_KEY"):
        sys.exit("set ANTHROPIC_API_KEY (mem0's extraction LLM needs it)")

    items = []
    with open(args.items, "r", encoding="utf-8") as fh:
        for line in fh:
            if line.strip():
                items.append(json.loads(line))
    if args.limit is not None:
        items = items[: args.limit]

    # Resume: skip items already ranked.
    done = set()
    if os.path.exists(args.out):
        with open(args.out, "r", encoding="utf-8") as fh:
            for line in fh:
                if line.strip():
                    done.add(json.loads(line)["query_id"])
    todo = [i for i in items if i["query_id"] not in done]
    print(f"{len(items)} items, {len(done)} done, {len(todo)} to go", file=sys.stderr)

    workdir = tempfile.mkdtemp(prefix="mem0_peer_")
    lock = threading.Lock()
    t_start = time.time()
    completed = 0

    with open(args.out, "a", encoding="utf-8") as out, ThreadPoolExecutor(
        max_workers=args.workers
    ) as pool:
        futures = {
            pool.submit(process_item, item, args.top_k, workdir, n % args.workers): item
            for n, item in enumerate(todo)
        }
        for fut in as_completed(futures):
            row = fut.result()
            extracted = row.pop("_memories_extracted")
            with lock:
                out.write(json.dumps(row) + "\n")
                out.flush()
                completed += 1
                elapsed = time.time() - t_start
                rate = completed / max(elapsed, 1e-9)
                eta = (len(todo) - completed) / max(rate, 1e-9)
                print(
                    f"  {completed}/{len(todo)} [{row['query_id']}] "
                    f"{extracted} memories, build {row['build_ns'] / 1e9:.0f}s, "
                    f"eta {eta / 60:.0f}m",
                    file=sys.stderr,
                    flush=True,
                )

    print(f"wrote rankings -> {args.out}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
