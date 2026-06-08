import json, sys, math

def load(path):
    rows=[]
    with open(path,'r',encoding='utf-8') as f:
        for line in f:
            line=line.strip()
            if line:
                rows.append(json.loads(line))
    return rows

def pct(vals,p):
    if not vals: return 0
    s=sorted(vals)
    rank=math.ceil((len(s)-1)*p/100.0)
    return s[min(rank,len(s)-1)]

def mean(vals):
    return sum(vals)/len(vals) if vals else 0

def analyze(path,label):
    rows=load(path)
    n=len(rows)
    r1=mean([r['scores']['recall_at_1'] for r in rows])
    r5=mean([r['scores']['recall_at_5'] for r in rows])
    r10=mean([r['scores']['recall_at_10'] for r in rows])
    ndcg10=mean([r['scores']['ndcg_at_10'] for r in rows])
    mrr=mean([r['scores']['mrr'] for r in rows])
    retr=[r['timings']['total_retrieval_ns'] for r in rows]
    build=[r['timings']['build_or_load_ns'] for r in rows]
    honest=[r['timings']['total_retrieval_ns']+r['timings']['build_or_load_ns'] for r in rows]
    print(f"\n=== {label}  (n={n}) ===")
    print(f"  Recall@1={r1:.4f}  Recall@5={r5:.4f}  Recall@10={r10:.4f}  NDCG@10={ndcg10:.4f}  MRR={mrr:.4f}")
    print(f"  REPORTED latency (total_retrieval_ns only):  P50={pct(retr,50)/1e6:.2f}ms  P95={pct(retr,95)/1e6:.2f}ms")
    print(f"  build_or_load_ns (excluded from report):     P50={pct(build,50)/1e6:.2f}ms  P95={pct(build,95)/1e6:.2f}ms")
    print(f"  HONEST per-query (build + retrieval):        P50={pct(honest,50)/1e6:.2f}ms  P95={pct(honest,95)/1e6:.2f}ms")
    return dict(rows=rows,r1=r1,r5=r5,r10=r10,ndcg10=ndcg10,mrr=mrr,
                retr_p50=pct(retr,50),build_p50=pct(build,50),honest_p50=pct(honest,50))

def compare_ranking(a,b):
    # check if returned_parent_ids identical per query
    ra={r['query_id']:r['returned_parent_ids'] for r in a}
    rb={r['query_id']:r['returned_parent_ids'] for r in b}
    common=set(ra)&set(rb)
    identical=sum(1 for q in common if ra[q]==rb[q])
    top1_same=sum(1 for q in common if ra[q][:1]==rb[q][:1])
    return identical,top1_same,len(common)

if __name__=='__main__':
    hp=analyze(sys.argv[1],sys.argv[2])
    bm=analyze(sys.argv[3],sys.argv[4])
    ident,t1,tot=compare_ranking(hp['rows'],bm['rows'])
    print(f"\n=== RANKING OVERLAP: {sys.argv[2]} vs {sys.argv[4]} ===")
    print(f"  identical full top-k ordering: {ident}/{tot} ({100*ident/tot:.1f}%)")
    print(f"  identical rank-1 result:       {t1}/{tot} ({100*t1/tot:.1f}%)")
    # honest speed verdict (a = first arg, b = second arg)
    a_label, b_label = sys.argv[2], sys.argv[4]
    print(f"\n=== LATENCY VERDICT ({a_label} vs {b_label}) ===")
    print(f"  Amortized hot-path (retrieval only): {a_label} {hp['retr_p50']/1e6:.2f}ms vs {b_label} {bm['retr_p50']/1e6:.2f}ms")
    print(f"  HONEST end-to-end (build+retrieval): {a_label} {hp['honest_p50']/1e6:.2f}ms vs {b_label} {bm['honest_p50']/1e6:.2f}ms")
    a, b = hp['honest_p50'], bm['honest_p50']
    print(f"  End-to-end: {a_label} is {b/a:.2f}x {'faster' if a<b else 'slower'} than {b_label}")
