"""Reciprocal-rank-fusion of two retrievers, scored with the exact metric in
crates/l5m-benchmarks/src/metrics.rs (recall@k = hits-in-top-k / |truth|).

Proves the Phase 5 thesis: L5M's lexical precision + dense embeddings' deep
recall, fused, beats either alone — and the real vector DB on deep recall.
"""
import json
import sys
import math


def load(path):
    rows = {}
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if line:
                r = json.loads(line)
                rows[r["query_id"]] = r
    return rows


def recall_at(truth, ranked, k):
    if not truth:
        return None
    hits = len({p for p in ranked[:k] if p in truth})
    return hits / len(truth)


def ndcg_at(truth, ranked, k):
    if not truth:
        return None
    dcg = sum(1.0 / math.log2(i + 2) for i, p in enumerate(ranked[:k]) if p in truth)
    ideal = sum(1.0 / math.log2(i + 2) for i in range(min(len(truth), k)))
    return dcg / ideal if ideal else 0.0


def mrr(truth, ranked):
    if not truth:
        return None
    for i, p in enumerate(ranked):
        if p in truth:
            return 1.0 / (i + 1)
    return 0.0


def rrf(rank_lists, k_rrf=60):
    scores = {}
    for ranked in rank_lists:
        for i, p in enumerate(ranked):
            scores[p] = scores.get(p, 0.0) + 1.0 / (k_rrf + i + 1)
    return [p for p, _ in sorted(scores.items(), key=lambda kv: -kv[1])]


def evaluate(name, get_ranked, ids, truths):
    agg = {"R@1": [], "R@5": [], "R@10": [], "NDCG@10": [], "MRR": []}
    for qid in ids:
        truth = truths[qid]
        if not truth:
            continue
        ranked = get_ranked(qid)
        agg["R@1"].append(recall_at(truth, ranked, 1))
        agg["R@5"].append(recall_at(truth, ranked, 5))
        agg["R@10"].append(recall_at(truth, ranked, 10))
        agg["NDCG@10"].append(ndcg_at(truth, ranked, 10))
        agg["MRR"].append(mrr(truth, ranked))
    out = {m: sum(v) / len(v) for m, v in agg.items()}
    print(
        f"{name:30} R@1={out['R@1']:.3f}  R@5={out['R@5']:.3f}  "
        f"R@10={out['R@10']:.3f}  NDCG@10={out['NDCG@10']:.3f}  MRR={out['MRR']:.3f}"
    )
    return out


if __name__ == "__main__":
    l5m = load(sys.argv[1])
    dense = load(sys.argv[2])
    ids = [q for q in l5m if q in dense]
    truths = {q: set(l5m[q]["ground_truth_ids"]) for q in ids}

    evaluate("L5M (lexical)", lambda q: l5m[q]["returned_parent_ids"], ids, truths)
    evaluate("Chroma+MiniLM (dense)", lambda q: dense[q]["returned_parent_ids"], ids, truths)
    evaluate(
        "HYBRID RRF (L5M + dense)",
        lambda q: rrf([l5m[q]["returned_parent_ids"], dense[q]["returned_parent_ids"]]),
        ids,
        truths,
    )
