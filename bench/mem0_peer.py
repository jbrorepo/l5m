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

# Fail-fast switch: a billing failure must abort the whole run immediately —
# continuing produces silently-degraded ingestion that poisons the benchmark
# (learned the hard way in run 1).
ABORT = threading.Event()

# Global pacing: mem0 1.x swallows some 429s internally ("Empty response from
# LLM") and silently stores nothing for that session — invisible to our retry
# wrapper. The only reliable fix is to stay UNDER the org TPM limit, so add()
# starts are globally rate-paced across all workers.
_PACE_LOCK = threading.Lock()
_NEXT_SLOT = [0.0]


def pace(min_interval: float) -> None:
    if min_interval <= 0:
        return
    with _PACE_LOCK:
        now = time.monotonic()
        slot = max(_NEXT_SLOT[0], now)
        _NEXT_SLOT[0] = slot + min_interval
    delay = slot - now
    if delay > 0:
        time.sleep(delay)


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


def process_item(item, top_k: int, workdir: str, worker: int, min_add_interval: float = 0.0):
    qid = item["query_id"]
    docs = item["documents"]
    memory = build_memory(workdir, worker)

    # BUILD: the real mem0 ingestion pipeline (LLM extraction per session).
    build_ns = 0
    extracted = 0
    add_errors = 0
    for doc in docs:
        if ABORT.is_set():
            return None
        text = doc["text"].strip()
        if not text:
            continue
        pace(min_add_interval)
        t0 = time.perf_counter_ns()
        for attempt in range(6):
            if ABORT.is_set():
                return None
            try:
                res = memory.add(
                    [{"role": "user", "content": text[:30000]}],
                    user_id=qid,
                    metadata={"capsule_id": doc["capsule_id"]},
                )
                extracted += len(res.get("results", []) or [])
                break
            except Exception as e:
                msg = str(e)
                if "credit balance is too low" in msg:
                    # Genuine billing exhaustion: continuing poisons the run.
                    print(f"  [{qid}] BILLING FAILURE — aborting run", file=sys.stderr)
                    ABORT.set()
                    return None
                if "rate_limit" in msg or "429" in msg or "overloaded" in msg.lower():
                    # TPM throttle — wait out the minute window and retry.
                    wait = min(30 * (attempt + 1), 120)
                    time.sleep(wait)
                    continue
                add_errors += 1
                print(f"  [{qid}] add error: {type(e).__name__}: {e}", file=sys.stderr)
                break
        else:
            add_errors += 1
            print(f"  [{qid}] add gave up after retries (rate limit)", file=sys.stderr)
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
        "add_errors": add_errors,
        "_memories_extracted": extracted,
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--items", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--top-k", type=int, default=10)
    ap.add_argument("--limit", type=int, default=None, help="cap number of items")
    ap.add_argument("--workers", type=int, default=4)
    ap.add_argument(
        "--workdir",
        default=None,
        help="persist mem0 stores here (default: fresh tempdir). Reuse for the QA phase.",
    )
    ap.add_argument(
        "--min-add-interval",
        type=float,
        default=0.0,
        help="global seconds between add() starts (stay under the org TPM limit)",
    )
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

    workdir = args.workdir or tempfile.mkdtemp(prefix="mem0_peer_")
    os.makedirs(workdir, exist_ok=True)
    print(f"mem0 stores -> {workdir}", file=sys.stderr)
    lock = threading.Lock()
    t_start = time.time()
    completed = 0

    with open(args.out, "a", encoding="utf-8") as out, ThreadPoolExecutor(
        max_workers=args.workers
    ) as pool:
        futures = {
            pool.submit(
                process_item, item, args.top_k, workdir, n % args.workers, args.min_add_interval
            ): item
            for n, item in enumerate(todo)
        }
        for fut in as_completed(futures):
            row = fut.result()
            if row is None:  # aborted (billing failure)
                continue
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
                    f"{extracted} memories, {row['add_errors']} errors, "
                    f"build {row['build_ns'] / 1e9:.0f}s, eta {eta / 60:.0f}m",
                    file=sys.stderr,
                    flush=True,
                )

    if ABORT.is_set():
        print("RUN ABORTED on billing failure — rankings are PARTIAL.", file=sys.stderr)
        return 2
    print(f"wrote rankings -> {args.out}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
