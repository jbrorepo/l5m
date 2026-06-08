#![forbid(unsafe_code)]

use std::{
    fs,
    io::{self, BufRead, Write},
    path::PathBuf,
};

use clap::{Parser, Subcommand};
use l5m_core::{
    compile_product_memories, compile_segment, compiler::parse_u128,
    segment_paths_from_product_dir, CompileOptions, L5mError, MemoryProbe, MemoryStore,
    ProductCompileOptions, QueryRequest, Result, Segment,
};

#[derive(Debug, Parser)]
#[command(name = "l5m", about = "Local low-latency 5D memory SDK and CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Init {
        #[arg(long, default_value = ".l5m")]
        dir: PathBuf,
    },
    Ingest {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 1)]
        epoch: u64,
    },
    Compile {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        epoch: u64,
    },
    Query {
        #[arg(long)]
        segment: PathBuf,
        #[arg(long)]
        request: Option<PathBuf>,
        #[arg(long)]
        tenant: Option<u64>,
        #[arg(long)]
        query: Option<String>,
        #[arg(long = "as-of")]
        as_of: Option<i64>,
        #[arg(long = "context-mask")]
        context_mask: Option<String>,
        #[arg(long = "policy-mask")]
        policy_mask: Option<String>,
        #[arg(long = "trust-floor")]
        trust_floor: Option<u8>,
        #[arg(long = "max-capsules", default_value_t = 8)]
        max_capsules: usize,
        #[arg(long = "max-tokens", default_value_t = 1024)]
        max_tokens: usize,
        #[arg(long = "include-supporting", default_value_t = false)]
        include_supporting: bool,
        #[arg(long = "include-contradictions", default_value_t = false)]
        include_contradictions: bool,
        #[arg(long = "max-hops", default_value_t = 1)]
        max_hops: u8,
    },
    Inspect {
        #[arg(long)]
        segment: PathBuf,
    },
    Validate {
        #[arg(long)]
        segment: PathBuf,
    },
    ServeStdio {
        #[arg(long)]
        segment: PathBuf,
    },
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    match Cli::parse().command {
        Command::Init { dir } => {
            fs::create_dir_all(dir.join("segments"))?;
            fs::create_dir_all(dir.join("runs"))?;
            fs::write(
                dir.join("config.json"),
                serde_json::to_string_pretty(&serde_json::json!({
                    "format": "l5m-local",
                    "version": 1,
                    "default_mode": "L5m"
                }))?,
            )?;
            Ok(())
        }
        Command::Ingest { input, out, epoch } => {
            let manifest = compile_product_memories(ProductCompileOptions {
                input_jsonl: input,
                output_dir: out,
                epoch,
            })?;
            println!("{}", serde_json::to_string_pretty(&manifest)?);
            Ok(())
        }
        Command::Compile {
            input,
            output,
            epoch,
        } => compile_segment(CompileOptions {
            input_json: input,
            output_segment: output,
            epoch,
        }),
        Command::Query {
            segment,
            request,
            tenant,
            query,
            as_of,
            context_mask,
            policy_mask,
            trust_floor,
            max_capsules,
            max_tokens,
            include_supporting,
            include_contradictions,
            max_hops,
        } => {
            if let Some(request) = request {
                let request: QueryRequest = serde_json::from_slice(&fs::read(request)?)?;
                let store = MemoryStore::open_segments([segment])?;
                let response = store.query(&request)?;
                println!("{}", serde_json::to_string_pretty(&response)?);
            } else {
                let frame = legacy_query(
                    segment,
                    tenant,
                    query,
                    as_of,
                    context_mask,
                    policy_mask,
                    trust_floor,
                    max_capsules,
                    max_tokens,
                    include_supporting,
                    include_contradictions,
                    max_hops,
                )?;
                println!("{}", serde_json::to_string_pretty(&frame)?);
            }
            Ok(())
        }
        Command::Inspect { segment } => {
            let segment = Segment::open(segment)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "epoch": segment.epoch(),
                    "tenant_id": segment.tenant_id(),
                    "capsule_count": segment.capsule_count()
                }))?
            );
            Ok(())
        }
        Command::Validate { segment } => {
            let segment = Segment::open(segment)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "valid": true,
                    "epoch": segment.epoch(),
                    "tenant_id": segment.tenant_id(),
                    "capsule_count": segment.capsule_count()
                }))?
            );
            Ok(())
        }
        Command::ServeStdio { segment } => {
            let paths = if segment.is_dir() {
                segment_paths_from_product_dir(&segment)?
            } else {
                vec![segment]
            };
            let store = MemoryStore::open_segments(paths)?;
            let stdin = io::stdin();
            let mut stdout = io::stdout().lock();
            for line in stdin.lock().lines() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                let request: QueryRequest = serde_json::from_str(&line)?;
                let response = store.query(&request)?;
                writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
                stdout.flush()?;
            }
            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn legacy_query(
    segment: PathBuf,
    tenant: Option<u64>,
    query: Option<String>,
    as_of: Option<i64>,
    context_mask: Option<String>,
    policy_mask: Option<String>,
    trust_floor: Option<u8>,
    max_capsules: usize,
    max_tokens: usize,
    include_supporting: bool,
    include_contradictions: bool,
    max_hops: u8,
) -> Result<l5m_core::MemoryFrame> {
    let tenant = tenant.ok_or_else(|| L5mError::Format("--tenant is required".to_string()))?;
    let query = query.ok_or_else(|| L5mError::Format("--query is required".to_string()))?;
    let as_of = as_of.ok_or_else(|| L5mError::Format("--as-of is required".to_string()))?;
    let context_mask =
        context_mask.ok_or_else(|| L5mError::Format("--context-mask is required".to_string()))?;
    let policy_mask =
        policy_mask.ok_or_else(|| L5mError::Format("--policy-mask is required".to_string()))?;
    let trust_floor =
        trust_floor.ok_or_else(|| L5mError::Format("--trust-floor is required".to_string()))?;

    let segment = Segment::open(segment)?;
    let mut probe = MemoryProbe::build(
        &query,
        tenant,
        as_of,
        parse_u128(&context_mask)?,
        parse_u128(&policy_mask)?,
        trust_floor,
    );
    probe.max_capsules = max_capsules;
    probe.max_tokens = max_tokens;
    probe.include_supporting = include_supporting;
    probe.include_contradictions = include_contradictions;
    probe.max_hops = max_hops;
    l5m_core::retrieve(&segment, &probe)
}
