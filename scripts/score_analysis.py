"""Rigorous score analysis: per-system means with 95% bootstrap CIs, paired
per-query deltas with significance (paired bootstrap), and per-category recall.

Usage:
  python scripts/score_analysis.py candidate=<label>=<run> other=<label>=<run> ...
The FIRST run is the candidate; pairwise deltas are candidate - each other.
"""
import json
import random
import sys

random.seed(12345)
METRICS = ["recall_at_1", "recall_at_5", "recall_at_10", "ndcg_at_10", "mrr"]
SHORT = {"recall_at_1": "R@1", "recall_at_5": "R@5", "recall_at_10": "R@10",
         "ndcg_at_10": "NDCG@10", "mrr": "MRR"}
B = 10000


def load(path):
    rows = {}
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if line:
                r = json.loads(line)
                rows[r["query_id"]] = r
    return rows


def mean(xs):
    return sum(xs) / len(xs) if xs else 0.0


def boot_ci(values, stat=mean):
    n = len(values)
    samples = []
    for _ in range(B):
        resample = [values[random.randrange(n)] for _ in range(n)]
        samples.append(stat(resample))
    samples.sort()
    return samples[int(0.025 * B)], samples[int(0.975 * B)]


def boot_delta(pairs):
    """pairs: list of (cand, other) per query -> mean delta + 95% CI + p(delta<=0)."""
    deltas = [c - o for c, o in pairs]
    n = len(deltas)
    md = mean(deltas)
    samples = []
    le0 = 0
    for _ in range(B):
        rs = [deltas[random.randrange(n)] for _ in range(n)]
        m = mean(rs)
        samples.append(m)
        if m <= 0:
            le0 += 1
    samples.sort()
    lo, hi = samples[int(0.025 * B)], samples[int(0.975 * B)]
    # two-sided bootstrap p: 2 * min(P(>0), P(<=0))
    p = 2 * min(le0 / B, 1 - le0 / B)
    return md, lo, hi, p


def parse(arg):
    # role=label=path  OR label=path
    parts = arg.split("=", 2)
    if len(parts) == 3:
        return parts[1], parts[2]
    return parts[0], parts[1]


def main():
    specs = [parse(a) for a in sys.argv[1:]]
    runs = [(label, load(path)) for label, path in specs]
    ids = set(runs[0][1])
    for _, r in runs[1:]:
        ids &= set(r)
    ids = sorted(ids)
    n = len(ids)
    print(f"# Score analysis  (n={n} queries, {B} bootstrap resamples, 95% CI)\n")

    print(f"{'system':32}" + "".join(f"{SHORT[m]:>20}" for m in METRICS))
    for label, rows in runs:
        cells = []
        for m in METRICS:
            vals = [rows[q]["scores"][m] for q in ids]
            lo, hi = boot_ci(vals)
            cells.append(f"{mean(vals):.3f} [{lo:.3f},{hi:.3f}]")
        print(f"{label:32}" + "".join(f"{c:>20}" for c in cells))

    cand_label, cand = runs[0]
    print(f"\n## Paired deltas vs '{cand_label}'  (positive = {cand_label} better)\n")
    for label, rows in runs[1:]:
        print(f"### {cand_label}  minus  {label}")
        for m in ["recall_at_5", "recall_at_10", "mrr"]:
            pairs = [(cand[q]["scores"][m], rows[q]["scores"][m]) for q in ids]
            md, lo, hi, p = boot_delta(pairs)
            wins = sum(1 for c, o in pairs if c > o)
            losses = sum(1 for c, o in pairs if c < o)
            ties = n - wins - losses
            sig = "SIGNIFICANT" if (lo > 0 or hi < 0) else "not significant"
            print(f"  {SHORT[m]:7} delta={md:+.4f}  95%CI[{lo:+.4f},{hi:+.4f}]  "
                  f"p~{p:.3f}  win/tie/loss={wins}/{ties}/{losses}  -> {sig}")
        print()

    # per-category R@10
    cats = sorted({rows[q]["category"] for q in ids for _, rows in [runs[0]]})
    print("## Per-category Recall@10\n")
    header = f"{'category':28}" + "".join(f"{lab[:16]:>18}" for lab, _ in runs) + f"{'n':>5}"
    print(header)
    by_cat_ids = {}
    for q in ids:
        by_cat_ids.setdefault(runs[0][1][q]["category"], []).append(q)
    for cat in sorted(by_cat_ids):
        qs = by_cat_ids[cat]
        cells = "".join(f"{mean([rows[q]['scores']['recall_at_10'] for q in qs]):>18.3f}" for _, rows in runs)
        print(f"{cat:28}{cells}{len(qs):>5}")


if __name__ == "__main__":
    main()
