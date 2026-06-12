#![forbid(unsafe_code)]

use std::{
    fs,
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use clap::Parser;
use l5m_core::{
    compile_segment, compiler::parse_u128, retrieve_with_timings, CompileOptions, L5mError,
    MemoryProbe, Result, RetrievalConfig, RetrievalTimings, Segment,
};
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Parser)]
struct Args {
    #[arg(long)]
    segment: Option<PathBuf>,
    #[arg(long)]
    queries: Option<PathBuf>,
    #[arg(long, default_value_t = 1000)]
    iterations: usize,
    #[arg(long, default_value_t = 0)]
    synthetic_capsules: usize,
    #[arg(long, default_value_t = 24)]
    synthetic_queries: usize,
    /// Override RetrievalConfig.ann_candidate_threshold (sublinear LSH path when
    /// the gated set exceeds this). Set very high to force the exact O(N) scan.
    #[arg(long)]
    ann_threshold: Option<usize>,
    /// Override RetrievalConfig.max_scored_candidates.
    #[arg(long)]
    max_scored: Option<usize>,
    /// Use specific "needle" queries (each targets one capsule) instead of the
    /// default broad whole-topic queries. Specific queries are the realistic
    /// memory-retrieval case and show the LSH index's sublinear behavior.
    #[arg(long)]
    needle: bool,
    /// Export the synthetic corpus + queries (with ground-truth target ids) as
    /// JSON so an external peer (e.g. a vector DB) can run the identical
    /// gated-retrieval workload. Only meaningful with --synthetic-capsules.
    #[arg(long)]
    export_corpus: Option<PathBuf>,
    /// Number of tenants to spread the synthetic corpus across. Each query is
    /// for one tenant, so the gate scan only touches that tenant's slice —
    /// demonstrates that tenant isolation (security) is also the latency win.
    #[arg(long, default_value_t = 1)]
    tenants: u64,
}

#[derive(Debug, Deserialize)]
struct QueryFixture {
    query: String,
    tenant: u64,
    as_of: i64,
    context_mask: String,
    policy_mask: String,
    trust_floor: u8,
    #[serde(default)]
    include_supporting: bool,
    #[serde(default)]
    include_contradictions: bool,
    /// Ground-truth capsule the (needle) query targets, enabling recall
    /// measurement alongside latency.
    #[serde(default)]
    target_capsule_id: Option<u128>,
    /// True when the target is legitimately ungated-out for this probe (low
    /// trust, expired/future validity, or policy mismatch): the CORRECT result
    /// is to not return it. Disclosing an embargoed target is a violation.
    #[serde(default)]
    target_embargoed: bool,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    if args.iterations == 0 {
        return Err(L5mError::Format(
            "iterations must be greater than zero".to_string(),
        ));
    }

    let (segment, probes, targets, synthetic_segment_path) = if args.synthetic_capsules > 0 {
        let (segment_path, fixtures, compile_ns) = generate_synthetic_segment(
            args.synthetic_capsules,
            args.synthetic_queries,
            args.needle,
            args.tenants.max(1),
            args.export_corpus.as_deref(),
        )?;
        let open_start = Instant::now();
        let segment = Segment::open(&segment_path)?;
        let open_ns = open_start.elapsed().as_nanos() as u64;
        let segment_bytes = fs::metadata(&segment_path).map(|m| m.len()).unwrap_or(0);
        println!("compile_ns: {compile_ns}");
        println!("open_ns: {open_ns}");
        println!("segment_bytes: {segment_bytes}");
        let (probes, targets) = build_probes(fixtures)?;
        (segment, probes, targets, Some(segment_path))
    } else {
        let segment_path = args.segment.ok_or_else(|| {
            L5mError::Format("missing --segment unless --synthetic-capsules is set".to_string())
        })?;
        let queries_path = args.queries.ok_or_else(|| {
            L5mError::Format("missing --queries unless --synthetic-capsules is set".to_string())
        })?;
        let segment = Segment::open(segment_path)?;
        let fixtures: Vec<QueryFixture> = serde_json::from_str(&fs::read_to_string(queries_path)?)?;
        let (probes, targets) = build_probes(fixtures)?;
        (segment, probes, targets, None)
    };
    if probes.is_empty() {
        return Err(L5mError::Format(
            "benchmark requires at least one query".to_string(),
        ));
    }

    if let Some(path) = &synthetic_segment_path {
        println!("synthetic_segment: {}", path.display());
        println!("synthetic_capsules: {}", segment.capsule_count());
        println!("synthetic_queries: {}", probes.len());
    }

    let mut config = RetrievalConfig::default();
    if let Some(threshold) = args.ann_threshold {
        config.ann_candidate_threshold = threshold;
    }
    if let Some(max_scored) = args.max_scored {
        config.max_scored_candidates = max_scored;
    }
    println!(
        "ann_candidate_threshold: {}",
        config.ann_candidate_threshold
    );
    println!("max_scored_candidates: {}", config.max_scored_candidates);
    let mut latencies = Vec::with_capacity(args.iterations);
    let mut candidate_total = 0usize;
    let mut returned_total = 0usize;
    let mut phase = RetrievalTimings::default();
    let mut needle_total = 0u64;
    let mut needle_hit_top1 = 0u64;
    let mut needle_hit_returned = 0u64;
    let mut embargoed_total = 0u64;
    let mut embargoed_disclosed = 0u64;
    for iteration in 0..args.iterations {
        let index = iteration % probes.len();
        let probe = &probes[index];
        let start = Instant::now();
        let (frame, timings) = retrieve_with_timings(&segment, probe, &config)?;
        latencies.push(start.elapsed().as_nanos() as u64);
        candidate_total += frame.coverage.candidate_count_before_scoring;
        returned_total += frame.capsules.len();
        phase.gate_filter_ns += timings.gate_filter_ns;
        phase.lookup_ns += timings.lookup_ns;
        phase.scoring_ns += timings.scoring_ns;
        phase.relation_ns += timings.relation_ns;
        // Recall / disclosure against the ground-truth target (first pass over
        // the query set only, so each query counts once).
        if iteration < probes.len() {
            if let (Some(target), embargoed) = targets[index] {
                let returned = frame.capsules.iter().any(|c| c.capsule_id == target);
                if embargoed {
                    // The CORRECT behavior is to refuse: the target fails a
                    // trust/temporal/policy gate for this probe.
                    embargoed_total += 1;
                    embargoed_disclosed += u64::from(returned);
                } else {
                    needle_total += 1;
                    if frame.capsules.first().map(|c| c.capsule_id) == Some(target) {
                        needle_hit_top1 += 1;
                    }
                    needle_hit_returned += u64::from(returned);
                }
            }
        }
    }
    let iters = args.iterations as u64;
    latencies.sort_unstable();
    let count = latencies.len();
    println!("iterations: {}", args.iterations);
    println!("p50_ns: {}", percentile(&latencies, 50));
    println!("p95_ns: {}", percentile(&latencies, 95));
    println!("p99_ns: {}", percentile(&latencies, 99));
    println!(
        "avg_candidate_count_before_scoring: {:.2}",
        candidate_total as f64 / count as f64
    );
    println!(
        "avg_returned_capsule_count: {:.2}",
        returned_total as f64 / count as f64
    );
    println!("phase_gate_filter_ns_avg: {}", phase.gate_filter_ns / iters);
    println!("phase_lookup_ns_avg: {}", phase.lookup_ns / iters);
    println!("phase_scoring_ns_avg: {}", phase.scoring_ns / iters);
    println!("phase_relation_ns_avg: {}", phase.relation_ns / iters);
    if needle_total > 0 {
        println!("needle_queries: {needle_total}");
        println!(
            "needle_recall_at1: {:.4}",
            needle_hit_top1 as f64 / needle_total as f64
        );
        println!(
            "needle_recall_returned: {:.4}",
            needle_hit_returned as f64 / needle_total as f64
        );
    }
    if embargoed_total > 0 {
        println!("embargoed_queries: {embargoed_total}");
        println!("embargoed_disclosed: {embargoed_disclosed}");
    }
    Ok(())
}

type Target = (Option<u128>, bool); // (target id, embargoed)

fn build_probes(fixtures: Vec<QueryFixture>) -> Result<(Vec<MemoryProbe>, Vec<Target>)> {
    let mut probes = Vec::with_capacity(fixtures.len());
    let mut targets = Vec::with_capacity(fixtures.len());
    for fixture in fixtures {
        let mut probe = MemoryProbe::build(
            &fixture.query,
            fixture.tenant,
            fixture.as_of,
            parse_u128(&fixture.context_mask)?,
            parse_u128(&fixture.policy_mask)?,
            fixture.trust_floor,
        );
        probe.include_supporting = fixture.include_supporting;
        probe.include_contradictions = fixture.include_contradictions;
        probes.push(probe);
        targets.push((fixture.target_capsule_id, fixture.target_embargoed));
    }
    Ok((probes, targets))
}

/// A broad vocabulary so synthetic capsules get *diverse* fingerprints (real
/// text does). Without this, every capsule's text is near-identical boilerplate,
/// fingerprints cluster, and any semantic index degenerates.
const VOCAB: &[&str] = &[
    "backup",
    "retention",
    "scanning",
    "incident",
    "payment",
    "audit",
    "deploy",
    "warehouse",
    "latency",
    "encryption",
    "rollback",
    "throughput",
    "policy",
    "cluster",
    "billing",
    "schema",
    "token",
    "quota",
    "replica",
    "snapshot",
    "failover",
    "ingest",
    "compaction",
    "tenant",
    "vector",
    "segment",
    "probe",
    "gate",
    "capsule",
    "fingerprint",
    "anchor",
    "evidence",
    "kafka",
    "postgres",
    "redis",
    "shard",
    "lambda",
    "cache",
    "queue",
    "webhook",
    "oauth",
    "metric",
    "trace",
    "alert",
    "runbook",
    "quorum",
    "leader",
    "consensus",
];

/// Deterministic per-capsule set of distinctive words.
fn seeded_words(index: usize, count: usize) -> String {
    let mut state = (index as u64).wrapping_mul(2654435761) ^ 0x9e37_79b9_7f4a_7c15;
    let mut words = Vec::with_capacity(count);
    for _ in 0..count {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        words.push(VOCAB[((state >> 33) as usize) % VOCAB.len()]);
    }
    words.join(" ")
}

fn generate_synthetic_segment(
    capsule_count: usize,
    query_count: usize,
    needle: bool,
    tenants: u64,
    export_corpus: Option<&std::path::Path>,
) -> Result<(PathBuf, Vec<QueryFixture>, u64)> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("l5m-bench-{now}"));
    fs::create_dir_all(&dir)?;
    let input = dir.join("synthetic.json");
    let segment = dir.join("synthetic.segment");

    let topics = [
        "backup retention",
        "aggressive scanning",
        "CVE mitigation",
        "audit logging",
        "deploy freeze",
        "payment timeout",
        "warehouse retention",
        "incident paging",
    ];
    let contexts = ["0x1", "0x2", "0x4", "0xffff"];
    let services = [
        "prod-db",
        "api-gateway",
        "prod-web-17",
        "billing",
        "search",
        "analytics",
        "checkout",
        "endpoint",
    ];

    let mut capsules = Vec::with_capacity(capsule_count);
    for index in 0..capsule_count {
        let topic = topics[index % topics.len()];
        let service = services[(index / topics.len()) % services.len()];
        let context_mask = contexts[index % contexts.len()];
        let tenant_id = if tenants > 1 {
            (index as u64 % tenants) + 1
        } else if index % 29 == 0 {
            2
        } else {
            1
        };
        let policy_mask = if index % 17 == 0 { "0x8" } else { "0xffff" };
        let trust_level = if index % 19 == 0 {
            2
        } else {
            5 + (index % 5) as u8
        };
        let control = format!("control-l5m-{index:05}");
        let mut capsule = json!({
            "capsule_id": (index + 1).to_string(),
            "tenant_id": tenant_id,
            "claim": format!("{topic} policy for {service} synthetic capsule {index} is governed by control L5M-{index:05}."),
            "evidence": format!("{} marker{index} for {service} covering {topic}", seeded_words(index, 14)),
            "source_id": 10_000 + index as u64,
            "source_uri": format!("synthetic://{service}/{topic}/{index}"),
            "valid_from": if index % 23 == 0 { 1_780_000_000i64 } else { 1_760_000_000i64 - index as i64 },
            "observed_at": 1_760_000_000i64 + index as i64,
            "last_verified_at": 1_768_000_000i64 + (index % 5000) as i64,
            "context_mask": context_mask,
            "policy_mask": policy_mask,
            "trust_level": trust_level,
            "classification": if policy_mask == "0x8" { 8 } else { 2 },
            "poison_risk": if trust_level < 4 { 2 } else { 0 },
            "anchors": [topic, service],
            "entities": [topic, service, control],
        });
        if index % 31 == 0 {
            capsule["valid_until"] = json!(1_761_000_000i64);
        }
        let mut relations = Vec::new();
        if index > 0 && index % 9 == 0 {
            relations.push(json!({
                "from": (index + 1).to_string(),
                "to": index.to_string(),
                "kind": "Supports",
                "weight": 60
            }));
        }
        if index > 2 && index % 37 == 0 {
            relations.push(json!({
                "from": (index + 1).to_string(),
                "to": (index - 1).to_string(),
                "kind": "Contradicts",
                "weight": 80
            }));
        }
        if !relations.is_empty() {
            capsule["relation_edges"] = json!(relations);
        }
        capsules.push(capsule);
    }

    fs::write(&input, serde_json::to_string_pretty(&capsules)?)?;
    let compile_start = Instant::now();
    compile_segment(CompileOptions {
        input_json: input,
        output_segment: segment.clone(),
        epoch: 1,
    })?;
    let compile_ns = compile_start.elapsed().as_nanos() as u64;

    let requested_queries = query_count.max(1);
    let stride = (capsule_count / requested_queries).max(1);
    let mut fixtures = Vec::with_capacity(requested_queries);
    for index in 0..requested_queries {
        let topic = topics[index % topics.len()];
        let service = services[(index * 3) % services.len()];
        // Needle queries name a specific capsule's unique control token so the
        // probe matches essentially one capsule (realistic memory retrieval).
        // Broad queries match a whole topic (worst case for any ANN index).
        let target = (index * stride) % capsule_count;
        let query = if needle {
            // Reconstruct the target capsule's distinctive words so the probe
            // matches essentially one capsule (a specific memory lookup).
            format!("{} marker{target}", seeded_words(target, 14))
        } else {
            format!("What is the {topic} policy for {service}?")
        };
        let tenant = if tenants > 1 {
            (target as u64 % tenants) + 1
        } else {
            1
        };
        fixtures.push(QueryFixture {
            query,
            tenant,
            as_of: 1_770_000_000,
            context_mask: if needle {
                "0xffff".to_string()
            } else {
                contexts[index % contexts.len()].to_string()
            },
            policy_mask: if index % 7 == 0 {
                "0x1".to_string()
            } else {
                "0xffff".to_string()
            },
            trust_floor: 4,
            include_supporting: index % 5 == 0,
            include_contradictions: index % 3 == 0,
            target_capsule_id: if needle {
                Some((target + 1) as u128)
            } else {
                None
            },
            // Mirror the generator's gate-relevant assignments: a target is
            // embargoed for this probe when it fails trust (synthetic trust 2 <
            // floor 4), validity (expired or future), or the probe's narrowed
            // policy mask. Keep in sync with the capsule construction above.
            target_embargoed: needle
                && (target % 19 == 0 // trust_level 2 < floor 4
                    || target % 31 == 0 // valid_until expired before as_of
                    || target % 23 == 0 // valid_from in the future
                    || (index % 7 == 0 && target % 17 == 0)), // policy 0x8 vs probe 0x1
        });
    }

    // Export the identical workload for external peers (vector DBs etc.):
    // same texts, same tenant assignment, same queries, same ground truth.
    if let Some(path) = export_corpus {
        let docs: Vec<serde_json::Value> = capsules
            .iter()
            .map(|c| {
                json!({
                    "capsule_id": c["capsule_id"],
                    "tenant_id": c["tenant_id"],
                    "text": format!("{} {}", c["claim"].as_str().unwrap_or(""),
                                              c["evidence"].as_str().unwrap_or("")),
                })
            })
            .collect();
        let queries: Vec<serde_json::Value> = fixtures
            .iter()
            .map(|f| {
                json!({
                    "query": f.query,
                    "tenant": f.tenant,
                    "target_capsule_id": f.target_capsule_id.map(|t| t.to_string()),
                    "target_embargoed": f.target_embargoed,
                })
            })
            .collect();
        fs::write(
            path,
            serde_json::to_string(&json!({"documents": docs, "queries": queries}))?,
        )?;
        println!("exported_corpus: {}", path.display());
    }

    Ok((segment, fixtures, compile_ns))
}

fn percentile(values: &[u64], percentile: usize) -> u64 {
    let index = ((values.len().saturating_sub(1)) * percentile) / 100;
    values[index]
}
