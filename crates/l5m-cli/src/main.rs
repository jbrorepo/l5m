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
use serde_json::{json, Value};

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
    McpStdio {
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
        Command::McpStdio { segment } => run_mcp_stdio(segment),
    }
}

fn run_mcp_stdio(segment: PathBuf) -> Result<()> {
    let paths = if segment.is_dir() {
        segment_paths_from_product_dir(&segment)?
    } else {
        vec![segment]
    };
    let store = MemoryStore::open_segments(paths.clone())?;
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(request) => handle_mcp_request(&store, &paths, request),
            Err(err) => json_rpc_error(Value::Null, -32700, format!("parse error: {err}")),
        };
        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }
    Ok(())
}

fn handle_mcp_request(store: &MemoryStore, paths: &[PathBuf], request: Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match method {
        "initialize" => json_rpc_result(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "l5m",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        ),
        "tools/list" => json_rpc_result(
            id,
            json!({
                "tools": [
                    {
                        "name": "l5m_query",
                        "description": "Query L5M admissible memory and return a proof-bearing QueryResponse.",
                        "inputSchema": query_input_schema()
                    },
                    {
                        "name": "l5m_inspect",
                        "description": "Inspect loaded L5M segment metadata.",
                        "inputSchema": empty_input_schema()
                    },
                    {
                        "name": "l5m_validate",
                        "description": "Validate loaded L5M segments and return metadata.",
                        "inputSchema": empty_input_schema()
                    }
                ]
            }),
        ),
        "tools/call" => handle_tool_call(
            store,
            paths,
            id,
            request.get("params").unwrap_or(&Value::Null),
        ),
        _ => json_rpc_error(id, -32601, format!("method not found: {method}")),
    }
}

fn handle_tool_call(store: &MemoryStore, paths: &[PathBuf], id: Value, params: &Value) -> Value {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let result = match name {
        "l5m_query" => serde_json::from_value::<QueryRequest>(arguments)
            .map_err(|err| format!("invalid l5m_query arguments: {err}"))
            .and_then(|request| store.query(&request).map_err(|err| err.to_string()))
            .and_then(|response| serde_json::to_value(response).map_err(|err| err.to_string())),
        "l5m_inspect" | "l5m_validate" => Ok(json!({
            "valid": true,
            "segment_count": paths.len(),
            "segment_metadata": store.segment_metadata()
        })),
        "" => Err("tool name is required".to_string()),
        _ => Err(format!("unknown tool: {name}")),
    };

    match result {
        Ok(value) => json_rpc_result(id, mcp_tool_result(value)),
        Err(message) => json_rpc_error(id, -32601, message),
    }
}

fn json_rpc_result(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn json_rpc_error(id: Value, code: i64, message: String) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}

fn mcp_tool_result(value: Value) -> Value {
    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string());
    json!({
        "content": [
            {
                "type": "text",
                "text": text
            }
        ],
        "structuredContent": value
    })
}

fn empty_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

fn query_input_schema() -> Value {
    json!({
        "type": "object",
        "required": [
            "query",
            "tenant_id",
            "as_of",
            "context_mask",
            "policy_mask",
            "trust_floor",
            "max_capsules",
            "max_tokens"
        ],
        "properties": {
            "query": { "type": "string" },
            "tenant_id": { "type": "integer" },
            "as_of": { "type": "integer" },
            "context_mask": { "type": "string" },
            "policy_mask": { "type": "string" },
            "trust_floor": { "type": "integer" },
            "max_capsules": { "type": "integer" },
            "max_tokens": { "type": "integer" },
            "include_supporting": { "type": "boolean" },
            "include_contradictions": { "type": "boolean" },
            "max_hops": { "type": "integer" },
            "mode": { "type": "string" }
        }
    })
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
