use std::{
    fs,
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use clap::Parser;
use l5m_core::{
    compile_segment, compiler::parse_u128, retrieve, CompileOptions, L5mError, MemoryProbe, Result,
    Segment,
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

    let (segment, probes, synthetic_segment_path) = if args.synthetic_capsules > 0 {
        let (segment_path, fixtures) =
            generate_synthetic_segment(args.synthetic_capsules, args.synthetic_queries)?;
        let segment = Segment::open(&segment_path)?;
        let probes = build_probes(fixtures)?;
        (segment, probes, Some(segment_path))
    } else {
        let segment_path = args.segment.ok_or_else(|| {
            L5mError::Format("missing --segment unless --synthetic-capsules is set".to_string())
        })?;
        let queries_path = args.queries.ok_or_else(|| {
            L5mError::Format("missing --queries unless --synthetic-capsules is set".to_string())
        })?;
        let segment = Segment::open(segment_path)?;
        let fixtures: Vec<QueryFixture> = serde_json::from_str(&fs::read_to_string(queries_path)?)?;
        let probes = build_probes(fixtures)?;
        (segment, probes, None)
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

    let mut latencies = Vec::with_capacity(args.iterations);
    let mut candidate_total = 0usize;
    let mut returned_total = 0usize;
    for iteration in 0..args.iterations {
        let probe = &probes[iteration % probes.len()];
        let start = Instant::now();
        let frame = retrieve(&segment, probe)?;
        latencies.push(start.elapsed().as_nanos() as u64);
        candidate_total += frame.coverage.candidate_count_before_scoring;
        returned_total += frame.capsules.len();
    }
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
    Ok(())
}

fn build_probes(fixtures: Vec<QueryFixture>) -> Result<Vec<MemoryProbe>> {
    let mut probes = Vec::with_capacity(fixtures.len());
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
    }
    Ok(probes)
}

fn generate_synthetic_segment(
    capsule_count: usize,
    query_count: usize,
) -> Result<(PathBuf, Vec<QueryFixture>)> {
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
        let tenant_id = if index % 29 == 0 { 2 } else { 1 };
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
            "evidence": format!("Synthetic evidence for {service} covering {topic}; generated to exercise retrieval gates, scoring, and relation expansion."),
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
    compile_segment(CompileOptions {
        input_json: input,
        output_segment: segment.clone(),
        epoch: 1,
    })?;

    let requested_queries = query_count.max(1);
    let mut fixtures = Vec::with_capacity(requested_queries);
    for index in 0..requested_queries {
        let topic = topics[index % topics.len()];
        let service = services[(index * 3) % services.len()];
        fixtures.push(QueryFixture {
            query: format!("What is the {topic} policy for {service}?"),
            tenant: 1,
            as_of: 1_770_000_000,
            context_mask: contexts[index % contexts.len()].to_string(),
            policy_mask: if index % 7 == 0 {
                "0x1".to_string()
            } else {
                "0xffff".to_string()
            },
            trust_floor: 4,
            include_supporting: index % 5 == 0,
            include_contradictions: index % 3 == 0,
        });
    }
    Ok((segment, fixtures))
}

fn percentile(values: &[u64], percentile: usize) -> u64 {
    let index = ((values.len().saturating_sub(1)) * percentile) / 100;
    values[index]
}
