#!/usr/bin/env python3
"""Gated-retrieval peer: a real vector DB (Chroma + MiniLM) on the identical
multi-tenant workload exported by `l5m-bench --export-corpus`.

What this measures (all local — zero API cost):

  1. filtered    — the vector DB done RIGHT: every query carries a
                   where={"tenant": t} metadata filter. Latency + recall on
                   clean needles + embargoed-target disclosures (a vector DB
                   has no trust/temporal/policy concept, so targets L5M
                   correctly refuses are disclosed here).
  2. unfiltered  — the same store when the application FORGETS the filter on
                   one code path (the classic multi-tenant RAG bug). Counts
                   cross-tenant rows in top-k: data another tenant should
                   never see.

The contrast with L5M: tenancy + trust + temporal + policy are enforced inside
the engine before scoring — there is no filter to forget, and embargoed
disclosures are structurally zero.
"""

import argparse
import json
import statistics
import sys
import time

import chromadb
from chromadb.config import Settings
from chromadb.utils import embedding_functions as ef


def pctl(values, p):
    values = sorted(values)
    return values[min(int(len(values) * p / 100), len(values) - 1)]


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", required=True, help="JSON from l5m-bench --export-corpus")
    ap.add_argument("--top-k", type=int, default=8)
    ap.add_argument("--out", default=None, help="optional JSON results file")
    args = ap.parse_args()

    data = json.load(open(args.corpus, encoding="utf-8"))
    docs = data["documents"]
    queries = data["queries"]
    print(f"{len(docs)} documents, {len(queries)} queries", file=sys.stderr)

    embed = ef.DefaultEmbeddingFunction()  # all-MiniLM-L6-v2, ONNX, local
    client = chromadb.Client(Settings(anonymized_telemetry=False, is_persistent=False))
    coll = client.create_collection(
        name="gated", embedding_function=embed, metadata={"hnsw:space": "cosine"}
    )

    # BUILD: embed + index everything, tenant as metadata.
    t0 = time.perf_counter_ns()
    batch = 2000
    for i in range(0, len(docs), batch):
        chunk = docs[i : i + batch]
        coll.add(
            ids=[str(d["capsule_id"]) for d in chunk],
            documents=[d["text"] or " " for d in chunk],
            metadatas=[{"tenant": int(d["tenant_id"])} for d in chunk],
        )
        print(f"  indexed {min(i + batch, len(docs))}/{len(docs)}", file=sys.stderr)
    build_ns = time.perf_counter_ns() - t0

    results = {"build_ns": build_ns, "documents": len(docs), "queries": len(queries)}

    for mode in ["filtered", "unfiltered"]:
        latencies = []
        clean_total = clean_at1 = clean_returned = 0
        embargoed_total = embargoed_disclosed = 0
        cross_tenant_rows = 0
        total_rows = 0
        for q in queries:
            where = {"tenant": int(q["tenant"])} if mode == "filtered" else None
            t1 = time.perf_counter_ns()
            res = coll.query(
                query_texts=[q["query"]],
                n_results=args.top_k,
                where=where,
                include=["metadatas"],
            )
            latencies.append(time.perf_counter_ns() - t1)
            ids = res["ids"][0] if res.get("ids") else []
            metas = res["metadatas"][0] if res.get("metadatas") else []

            total_rows += len(ids)
            cross_tenant_rows += sum(
                1 for m in metas if int(m.get("tenant", -1)) != int(q["tenant"])
            )

            target = q.get("target_capsule_id")
            if target is None:
                continue
            target = str(target)
            if q.get("target_embargoed"):
                embargoed_total += 1
                embargoed_disclosed += int(target in ids)
            else:
                clean_total += 1
                clean_returned += int(target in ids)
                clean_at1 += int(bool(ids) and ids[0] == target)

        results[mode] = {
            "query_p50_ms": pctl(latencies, 50) / 1e6,
            "query_p95_ms": pctl(latencies, 95) / 1e6,
            "clean_needles": clean_total,
            "recall_at1": clean_at1 / clean_total if clean_total else None,
            "recall_at_k": clean_returned / clean_total if clean_total else None,
            "embargoed_queries": embargoed_total,
            "embargoed_disclosed": embargoed_disclosed,
            "cross_tenant_rows_in_topk": cross_tenant_rows,
            "total_rows_returned": total_rows,
        }
        r = results[mode]
        print(
            f"\n=== {mode} ===\n"
            f"  query p50 {r['query_p50_ms']:.2f} ms  p95 {r['query_p95_ms']:.2f} ms\n"
            f"  clean recall@1 {r['recall_at1']:.3f}  recall@{args.top_k} {r['recall_at_k']:.3f}"
            f"  (n={clean_total})\n"
            f"  embargoed disclosed: {embargoed_disclosed}/{embargoed_total}\n"
            f"  cross-tenant rows in top-k: {cross_tenant_rows}/{total_rows}"
        )

    print(f"\nbuild: {build_ns / 1e9:.1f}s for {len(docs)} docs", file=sys.stderr)
    if args.out:
        with open(args.out, "w", encoding="utf-8") as f:
            json.dump(results, f, indent=2)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
