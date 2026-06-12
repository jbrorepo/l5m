#!/usr/bin/env python3
"""QA-accuracy head-to-head: L5M vs Mem0 on LongMemEval dev-50.

The fair protocol for comparing a ranking-based memory (L5M) with a
consolidation-based one (Mem0): each system supplies its retrieved context,
the SAME answerer model generates an answer from that context, and the SAME
judge scores it against gold. No source attribution required — this measures
what both products actually promise: answer the question from memory.

  - Answerer: claude-haiku-4-5, temperature 0, identical prompt per system.
  - Judge:    claude-haiku-4-5, gold-reference comparison, identical prompt.
  - L5M context: top-k retrieved sessions (returned_capsule_ids from the
    standard run file -> session text from items.jsonl).
  - Mem0 context: top memories from mem0's own search over its persisted
    stores (memories are mem0's product surface — that's what a mem0 app
    injects into the prompt).
  - Paired per-question results; significance via paired bootstrap on the
    accuracy difference.

Context-size asymmetry is inherent and reported, not hidden: mem0 injects
terse distilled facts (cheap, lossy); L5M injects full sessions (bigger,
complete). Each system is evaluated at its real operating point.
"""

import argparse
import json
import os
import random
import sys
from concurrent.futures import ThreadPoolExecutor

ANSWER_MODEL = "claude-haiku-4-5"
JUDGE_MODEL = "claude-haiku-4-5"

ANSWER_PROMPT = """\
You are answering a question using ONLY the memory context below. If the
context does not contain the information needed, reply exactly: I don't know.

Memory context:
{context}

Question ({question_date}): {question}

Answer concisely.
"""

JUDGE_PROMPT = """\
Judge whether the model's answer is correct given the gold answer.
The answer is correct if it conveys the same essential information as the
gold answer; phrasing may differ. If the gold answer indicates the question
is unanswerable/has no answer, then "I don't know" (or equivalent abstention)
is correct.

Question: {question}
Gold answer: {gold}
Model answer: {answer}

Reply with exactly one word: correct or incorrect.
"""


def client():
    import anthropic

    return anthropic.Anthropic()


def ask(c, prompt: str, max_tokens: int = 300) -> str:
    resp = c.messages.create(
        model=ANSWER_MODEL,
        max_tokens=max_tokens,
        temperature=0.0,
        messages=[{"role": "user", "content": prompt}],
    )
    return next((b.text for b in resp.content if b.type == "text"), "").strip()


def judge(c, question: str, gold: str, answer: str) -> bool:
    resp = c.messages.create(
        model=JUDGE_MODEL,
        max_tokens=10,
        temperature=0.0,
        messages=[
            {
                "role": "user",
                "content": JUDGE_PROMPT.format(question=question, gold=gold, answer=answer),
            }
        ],
    )
    text = next((b.text for b in resp.content if b.type == "text"), "").strip().lower()
    return text.startswith("correct")


def l5m_context(run_row, docs_by_id, k: int) -> str:
    ids = [str(i) for i in run_row.get("returned_capsule_ids", [])][:k]
    parts = [docs_by_id[i] for i in ids if i in docs_by_id]
    return "\n\n--- session ---\n".join(parts)


def mem0_context(memories, qid: str, question: str, k: int) -> str:
    """Search every worker store (an item's memories live in exactly one, but
    resume runs make the assignment non-deterministic); merge by score."""
    hits = []
    for memory in memories:
        try:
            res = memory.search(question, user_id=qid, limit=k)
        except Exception:
            continue
        hits.extend(res.get("results", []) or [])
    hits.sort(key=lambda h: h.get("score") or 0.0, reverse=True)
    facts = []
    for hit in hits[:k]:
        text = hit.get("memory") or (hit.get("metadata") or {}).get("data") or ""
        if text:
            facts.append(f"- {text}")
    return "\n".join(facts)


def build_memories(workdir: str, workers: int):
    """One Memory handle per worker store (search-only; no LLM calls made by
    search except none — embedding is local fastembed)."""
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    from mem0_peer import build_memory

    return [build_memory(workdir, w) for w in range(workers)]


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--items", required=True)
    ap.add_argument("--dataset", required=True, help="longmemeval json (gold answers)")
    ap.add_argument("--l5m-run", required=True)
    ap.add_argument("--mem0-workdir", required=True)
    ap.add_argument("--mem0-workers", type=int, default=6)
    ap.add_argument("--top-k", type=int, default=10)
    ap.add_argument("--out", required=True)
    ap.add_argument("--llm-workers", type=int, default=8)
    args = ap.parse_args()

    if not os.environ.get("ANTHROPIC_API_KEY"):
        sys.exit("set ANTHROPIC_API_KEY")

    items = {i["query_id"]: i for i in map(json.loads, open(args.items, encoding="utf-8"))}
    gold = {
        q["question_id"]: q
        for q in json.load(open(args.dataset, encoding="utf-8"))
        if q["question_id"] in items
    }
    l5m_rows = {
        r["query_id"]: r for r in map(json.loads, open(args.l5m_run, encoding="utf-8"))
    }
    memories = build_memories(args.mem0_workdir, args.mem0_workers)
    qids = list(items.keys())

    c = client()
    results = []

    def eval_one(qid: str):
        item = items[qid]
        g = gold[qid]
        docs_by_id = {d["capsule_id"]: d["text"] for d in item["documents"]}

        ctx_l5m = l5m_context(l5m_rows[qid], docs_by_id, args.top_k)
        ctx_mem0 = mem0_context(memories, qid, item["question"], args.top_k)

        row = {"query_id": qid, "question_type": g.get("question_type")}
        for name, ctx in [("l5m", ctx_l5m), ("mem0", ctx_mem0)]:
            if not ctx.strip():
                answer = "I don't know"
            else:
                answer = ask(
                    c,
                    ANSWER_PROMPT.format(
                        context=ctx,
                        question=item["question"],
                        question_date=g.get("question_date", ""),
                    ),
                )
            correct = judge(c, item["question"], str(g["answer"]), answer)
            row[f"{name}_answer"] = answer
            row[f"{name}_correct"] = bool(correct)
            row[f"{name}_context_chars"] = len(ctx)
        return row

    with ThreadPoolExecutor(max_workers=args.llm_workers) as pool:
        for n, row in enumerate(pool.map(eval_one, qids), 1):
            results.append(row)
            print(
                f"  {n}/{len(qids)} [{row['query_id']}] "
                f"l5m={'Y' if row['l5m_correct'] else 'n'} "
                f"mem0={'Y' if row['mem0_correct'] else 'n'}",
                file=sys.stderr,
                flush=True,
            )

    with open(args.out, "w", encoding="utf-8") as f:
        for row in results:
            f.write(json.dumps(row) + "\n")

    # ---- summary + paired bootstrap ----
    n = len(results)
    acc_l5m = sum(r["l5m_correct"] for r in results) / n
    acc_mem0 = sum(r["mem0_correct"] for r in results) / n
    diffs = [int(r["l5m_correct"]) - int(r["mem0_correct"]) for r in results]

    rng = random.Random(42)
    boots = []
    for _ in range(10000):
        sample = [diffs[rng.randrange(n)] for _ in range(n)]
        boots.append(sum(sample) / n)
    boots.sort()
    lo, hi = boots[int(0.025 * len(boots))], boots[int(0.975 * len(boots))]
    p_le_zero = sum(1 for b in boots if b <= 0) / len(boots)

    med = lambda xs: sorted(xs)[len(xs) // 2]
    print("\n=== QA accuracy (LongMemEval dev-50, answerer+judge: haiku-4-5) ===")
    print(f"  L5M  : {acc_l5m:.3f}  (median context {med([r['l5m_context_chars'] for r in results])} chars)")
    print(f"  Mem0 : {acc_mem0:.3f}  (median context {med([r['mem0_context_chars'] for r in results])} chars)")
    print(f"  paired delta = {acc_l5m - acc_mem0:+.3f}  95% CI [{lo:+.3f}, {hi:+.3f}]  p(delta<=0) ~ {p_le_zero:.4f}")
    by_type = {}
    for r in results:
        by_type.setdefault(r["question_type"], []).append(r)
    print("  by question type (L5M vs Mem0):")
    for t, rows in sorted(by_type.items()):
        print(
            f"    {t:28s} n={len(rows):3d}  "
            f"{sum(x['l5m_correct'] for x in rows) / len(rows):.2f} vs "
            f"{sum(x['mem0_correct'] for x in rows) / len(rows):.2f}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
