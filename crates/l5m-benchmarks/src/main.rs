#![forbid(unsafe_code)]

mod adapters;
mod audit;
mod latency;
mod metrics;
mod modes;
mod report;
mod runfile;
mod safety;
mod split;

use std::{
    fs,
    path::{Path, PathBuf},
};

use adapters::{convomem, locomo, longmemeval, BenchmarkItem};
use clap::{Args, Parser, Subcommand, ValueEnum};
use modes::{validate_top_k, Mode};
use runfile::{decode_jsonl, encode_jsonl};
use split::{create_split, filter_items, SplitFile};

type AppResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Parser)]
#[command(name = "l5m-benchmarks")]
#[command(about = "MemPalace-style retrieval benchmarks for L5M")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Longmemeval(LongMemEvalArgs),
    Locomo(LoCoMoArgs),
    Convomem(ConvoMemArgs),
    Compare(CompareArgs),
    Audit(AuditArgs),
    ExplainMiss(ExplainMissArgs),
    Scorecard(ScorecardArgs),
    Diagnose(DiagnoseArgs),
    Prove(ProveArgs),
    Safety(SafetyArgs),
    /// Export resolved benchmark items (query + documents) for an external
    /// retriever (e.g. a vector-DB peer) to embed and rank.
    ExportItems(ExportItemsArgs),
    /// Score an external retriever's ranking through the identical harness path.
    ExternalRun(ExternalRunArgs),
    /// Reciprocal-rank-fuse two or more run files into a hybrid run, re-scored
    /// through the identical harness metrics.
    FuseRuns(FuseRunsArgs),
    /// Native hybrid run: compile dense embeddings into the segment + query
    /// vector on the probe, fuse lexical ⊕ dense inside L5M's own retrieval.
    EmbedRun(EmbedRunArgs),
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum BenchKind {
    Longmemeval,
    Convomem,
    Locomo,
}

#[derive(Debug, Args)]
struct LongMemEvalArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long, value_enum)]
    mode: Mode,
    #[arg(long, default_value_t = 10)]
    top_k: usize,
    #[arg(long)]
    out: PathBuf,
    #[arg(long)]
    create_split: bool,
    #[arg(long)]
    split_file: Option<PathBuf>,
    #[arg(long, default_value_t = 50)]
    dev_size: usize,
    #[arg(long, default_value_t = 42)]
    seed: u64,
    #[arg(long)]
    dev_only: bool,
    #[arg(long)]
    held_out: bool,
    #[arg(long)]
    config_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct LoCoMoArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long, value_enum)]
    mode: Mode,
    #[arg(long, value_enum, default_value_t = locomo::Granularity::Session)]
    granularity: locomo::Granularity,
    #[arg(long, default_value_t = 10)]
    top_k: usize,
    #[arg(long)]
    out: PathBuf,
    #[arg(long)]
    strict_top_k: bool,
    #[arg(long)]
    config_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ConvoMemArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long, value_enum)]
    mode: Mode,
    #[arg(long, value_delimiter = ',', default_value = "all")]
    categories: Vec<String>,
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long, default_value_t = 10)]
    top_k: usize,
    #[arg(long)]
    out: PathBuf,
    #[arg(long, value_enum, default_value_t = convomem::ConvoMemLayout::Auto)]
    convomem_layout: convomem::ConvoMemLayout,
    #[arg(long)]
    config_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct CompareArgs {
    #[arg(long, value_delimiter = ',')]
    runs: Vec<PathBuf>,
    #[arg(long)]
    out: PathBuf,
}

#[derive(Debug, Args)]
struct AuditArgs {
    #[arg(long)]
    run: PathBuf,
    #[arg(long)]
    out: PathBuf,
}

#[derive(Debug, Args)]
struct ExplainMissArgs {
    #[arg(long)]
    run: PathBuf,
    #[arg(long)]
    query_id: String,
    #[arg(long)]
    out: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ScorecardArgs {
    #[arg(long)]
    run: PathBuf,
    #[arg(long, value_enum)]
    preset: ScorecardPreset,
    #[arg(long)]
    out: PathBuf,
    #[arg(long, value_enum, default_value_t = ScorecardFormat::Markdown)]
    format: ScorecardFormat,
}

#[derive(Debug, Args)]
struct DiagnoseArgs {
    #[arg(long)]
    run: PathBuf,
    #[arg(long)]
    out: PathBuf,
}

#[derive(Debug, Args)]
struct ProveArgs {
    #[arg(long)]
    candidate: PathBuf,
    #[arg(long)]
    baseline: PathBuf,
    #[arg(long)]
    out: PathBuf,
    #[arg(long, value_enum, default_value_t = ScorecardFormat::Markdown)]
    format: ScorecardFormat,
}

#[derive(Debug, Args)]
struct SafetyArgs {
    #[arg(long)]
    run: PathBuf,
    #[arg(long)]
    out: PathBuf,
}

#[derive(Debug, Args)]
struct ExportItemsArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long, value_enum)]
    benchmark: BenchKind,
    #[arg(long)]
    out: PathBuf,
    #[arg(long)]
    split_file: Option<PathBuf>,
    #[arg(long)]
    dev_only: bool,
    #[arg(long)]
    held_out: bool,
    #[arg(long, value_delimiter = ',', default_value = "all")]
    categories: Vec<String>,
    #[arg(long)]
    limit: Option<usize>,
}

#[derive(Debug, Args)]
struct ExternalRunArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long, value_enum)]
    benchmark: BenchKind,
    /// JSONL: {"query_id","ranked_capsule_ids":[..],"build_ns","query_ns"} per line.
    #[arg(long)]
    rankings: PathBuf,
    #[arg(long, default_value = "vector-db")]
    mode_label: String,
    #[arg(long, default_value_t = 10)]
    top_k: usize,
    #[arg(long)]
    out: PathBuf,
    #[arg(long)]
    split_file: Option<PathBuf>,
    #[arg(long)]
    dev_only: bool,
    #[arg(long)]
    held_out: bool,
    #[arg(long, value_delimiter = ',', default_value = "all")]
    categories: Vec<String>,
    #[arg(long)]
    limit: Option<usize>,
}

#[derive(Debug, Args)]
struct EmbedRunArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long, value_enum)]
    benchmark: BenchKind,
    /// embeddings.jsonl from bench/emit_embeddings.py
    #[arg(long)]
    embeddings: PathBuf,
    #[arg(long, default_value = "l5m-hybrid-embed")]
    mode_label: String,
    #[arg(long, default_value_t = 10)]
    top_k: usize,
    /// Parent-aggregate the fused capsules (matches the `hybrid-parent` mode).
    #[arg(long)]
    parent_aggregate: bool,
    #[arg(long)]
    out: PathBuf,
    #[arg(long)]
    split_file: Option<PathBuf>,
    #[arg(long)]
    dev_only: bool,
    #[arg(long)]
    held_out: bool,
    #[arg(long, value_delimiter = ',', default_value = "all")]
    categories: Vec<String>,
    #[arg(long)]
    limit: Option<usize>,
}

#[derive(Debug, Args)]
struct FuseRunsArgs {
    /// Two or more run files to fuse (e.g. an L5M lexical run and a dense run).
    #[arg(long, value_delimiter = ',')]
    runs: Vec<PathBuf>,
    #[arg(long)]
    out: PathBuf,
    #[arg(long, default_value_t = 10)]
    top_k: usize,
    #[arg(long, default_value = "hybrid-rrf")]
    mode_label: String,
    /// Reciprocal-rank-fusion constant (k). Higher = flatter rank weighting.
    #[arg(long, default_value_t = 60.0)]
    k_rrf: f64,
}

#[derive(Clone, Debug, ValueEnum)]
enum ScorecardPreset {
    #[value(name = "mempalace-longmemeval")]
    Longmemeval,
    #[value(name = "mempalace-locomo")]
    Locomo,
    #[value(name = "mempalace-convomem")]
    Convomem,
}

impl ScorecardPreset {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Longmemeval => "mempalace-longmemeval",
            Self::Locomo => "mempalace-locomo",
            Self::Convomem => "mempalace-convomem",
        }
    }
}

#[derive(Clone, Debug, ValueEnum)]
enum ScorecardFormat {
    Markdown,
    Json,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> AppResult<()> {
    match Cli::parse().command {
        Commands::Longmemeval(args) => run_longmemeval(args),
        Commands::Locomo(args) => run_locomo(args),
        Commands::Convomem(args) => run_convomem(args),
        Commands::Compare(args) => run_compare(args),
        Commands::Audit(args) => run_audit(args),
        Commands::ExplainMiss(args) => run_explain_miss(args),
        Commands::Scorecard(args) => run_scorecard(args),
        Commands::Diagnose(args) => run_diagnose(args),
        Commands::Prove(args) => run_prove(args),
        Commands::Safety(args) => run_safety(args),
        Commands::ExportItems(args) => run_export_items(args),
        Commands::ExternalRun(args) => run_external_run(args),
        Commands::FuseRuns(args) => run_fuse_runs(args),
        Commands::EmbedRun(args) => run_embed_run(args),
    }
}

fn run_longmemeval(args: LongMemEvalArgs) -> AppResult<()> {
    if args.held_out {
        require_frozen_config(args.config_file.as_deref())?;
    }
    let identity = RunIdentity::new(
        &args.input,
        args.split_file.as_deref(),
        args.config_file.as_deref(),
    )?;
    let mut items = longmemeval::parse_path(&args.input)?;
    if args.create_split {
        let split_file = args
            .split_file
            .as_ref()
            .ok_or("--create-split requires --split-file")?;
        write_split(split_file, &items, args.dev_size, args.seed)?;
    }
    if args.dev_only || args.held_out {
        let split_file = args
            .split_file
            .as_ref()
            .ok_or("--dev-only/--held-out requires --split-file")?;
        let split = read_split(split_file)?;
        items = filter_items(items, &split, args.dev_only, args.held_out);
    }
    let mut rows = run_items(&items, args.mode, args.top_k)?;
    annotate_rows(&mut rows, &identity);
    write_runfile(&args.out, &rows)?;
    Ok(())
}

fn run_locomo(args: LoCoMoArgs) -> AppResult<()> {
    let identity = RunIdentity::new(&args.input, None, args.config_file.as_deref())?;
    let items = locomo::parse_path(&args.input, args.granularity)?;
    let mut rows = Vec::new();
    for item in &items {
        let decision = validate_top_k(args.top_k, item.documents.len(), args.strict_top_k)
            .map_err(|err| format!("{} query_id={}", err, item.query_id))?;
        if let Some(warning) = decision.warning {
            eprintln!("warning: {warning} query_id={}", item.query_id);
        }
        rows.push(modes::run_item(item, args.mode, decision.top_k)?);
    }
    annotate_rows(&mut rows, &identity);
    write_runfile(&args.out, &rows)?;
    Ok(())
}

fn run_convomem(args: ConvoMemArgs) -> AppResult<()> {
    let identity = RunIdentity::new(&args.input, None, args.config_file.as_deref())?;
    for category in &args.categories {
        let normalized = adapters::normalize_category(category);
        if normalized != "all" && !convomem::supported_categories().contains(&normalized.as_str()) {
            return Err(format!("unsupported ConvoMem category: {category}").into());
        }
    }
    let items = convomem::parse_path(
        &args.input,
        &args.categories,
        args.limit,
        args.convomem_layout,
    )?;
    let mut rows = run_items(&items, args.mode, args.top_k)?;
    for (row, item) in rows.iter_mut().zip(items.iter()) {
        apply_convomem_abstention(row, item);
    }
    annotate_rows(&mut rows, &identity);
    write_runfile(&args.out, &rows)?;
    Ok(())
}

/// Apply ConvoMem's abstention scoring override to a row (success when the
/// retriever returns nothing or an explicit insufficient-evidence marker).
/// Shared by `run_convomem` and the external vector-DB peer so both are scored
/// identically on abstention items.
fn apply_convomem_abstention(row: &mut runfile::RunRow, item: &BenchmarkItem) {
    if !item.abstention {
        return;
    }
    let returned = row
        .returned_parent_ids
        .iter()
        .map(|parent_id| modes::ReturnedCapsule {
            capsule_id: parent_id.clone(),
            parent_id: parent_id.clone(),
            token_estimate: 0,
        })
        .collect::<Vec<_>>();
    let success = convomem::score_abstention(&returned);
    row.scores.recall_at_1 = f64::from(success);
    row.scores.recall_at_5 = f64::from(success);
    row.scores.recall_at_10 = f64::from(success);
    row.scores.ndcg_at_5 = f64::from(success);
    row.scores.ndcg_at_10 = f64::from(success);
    row.scores.mrr = f64::from(success);
    row.scores.zero_recall = !success;
}

fn run_compare(args: CompareArgs) -> AppResult<()> {
    let mut runs = Vec::new();
    for path in &args.runs {
        let rows = decode_jsonl(&fs::read_to_string(path)?)?;
        runs.push((path.display().to_string(), rows));
    }
    write_parent(&args.out)?;
    fs::write(args.out, report::render_compare_markdown(&runs))?;
    Ok(())
}

fn run_audit(args: AuditArgs) -> AppResult<()> {
    let rows = decode_jsonl(&fs::read_to_string(args.run)?)?;
    write_parent(&args.out)?;
    fs::write(args.out, audit::render_audit_markdown(&rows))?;
    Ok(())
}

fn run_explain_miss(args: ExplainMissArgs) -> AppResult<()> {
    let rows = decode_jsonl(&fs::read_to_string(args.run)?)?;
    let explanation = audit::explain_miss(&rows, &args.query_id)
        .ok_or_else(|| format!("query_id not found: {}", args.query_id))?;
    if let Some(out) = args.out {
        write_parent(&out)?;
        fs::write(out, explanation)?;
    } else {
        println!("{explanation}");
    }
    Ok(())
}

fn run_scorecard(args: ScorecardArgs) -> AppResult<()> {
    let rows = decode_jsonl(&fs::read_to_string(args.run)?)?;
    validate_preset_rows(&args.preset, &rows)?;
    let rendered = match args.format {
        ScorecardFormat::Markdown => report::render_scorecard_markdown(args.preset.as_str(), &rows),
        ScorecardFormat::Json => report::render_scorecard_json(args.preset.as_str(), &rows)?,
    };
    write_parent(&args.out)?;
    fs::write(args.out, rendered)?;
    Ok(())
}

fn run_diagnose(args: DiagnoseArgs) -> AppResult<()> {
    let rows = decode_jsonl(&fs::read_to_string(args.run)?)?;
    write_parent(&args.out)?;
    fs::write(args.out, audit::render_diagnose_markdown(&rows))?;
    Ok(())
}

fn run_prove(args: ProveArgs) -> AppResult<()> {
    let candidate = decode_jsonl(&fs::read_to_string(&args.candidate)?)?;
    let baseline = decode_jsonl(&fs::read_to_string(&args.baseline)?)?;
    let candidate_name = args.candidate.display().to_string();
    let baseline_name = args.baseline.display().to_string();
    let rendered = match args.format {
        ScorecardFormat::Markdown => {
            report::render_proof_markdown(&candidate_name, &candidate, &baseline_name, &baseline)
        }
        ScorecardFormat::Json => {
            report::render_proof_json(&candidate_name, &candidate, &baseline_name, &baseline)?
        }
    };
    write_parent(&args.out)?;
    fs::write(args.out, rendered)?;
    Ok(())
}

fn run_safety(args: SafetyArgs) -> AppResult<()> {
    let rows = decode_jsonl(&fs::read_to_string(args.run)?)?;
    write_parent(&args.out)?;
    fs::write(args.out, safety::render_safety_markdown(&rows))?;
    Ok(())
}

fn load_items(
    kind: BenchKind,
    input: &Path,
    split_file: Option<&Path>,
    dev_only: bool,
    held_out: bool,
    categories: &[String],
    limit: Option<usize>,
) -> AppResult<Vec<BenchmarkItem>> {
    match kind {
        BenchKind::Longmemeval => {
            let mut items = longmemeval::parse_path(input)?;
            if dev_only || held_out {
                let split_file = split_file.ok_or("--dev-only/--held-out requires --split-file")?;
                let split = read_split(split_file)?;
                items = filter_items(items, &split, dev_only, held_out);
            }
            Ok(items)
        }
        BenchKind::Convomem => Ok(convomem::parse_path(
            input,
            categories,
            limit,
            convomem::ConvoMemLayout::Auto,
        )?),
        BenchKind::Locomo => Ok(locomo::parse_path(input, locomo::Granularity::Session)?),
    }
}

#[derive(serde::Deserialize)]
struct ExternalRanking {
    query_id: String,
    #[serde(default)]
    ranked_capsule_ids: Vec<String>,
    #[serde(default)]
    build_ns: u64,
    #[serde(default)]
    query_ns: u64,
}

fn run_export_items(args: ExportItemsArgs) -> AppResult<()> {
    let items = load_items(
        args.benchmark,
        &args.input,
        args.split_file.as_deref(),
        args.dev_only,
        args.held_out,
        &args.categories,
        args.limit,
    )?;
    write_parent(&args.out)?;
    let mut out = String::new();
    for item in &items {
        let documents = item
            .documents
            .iter()
            .map(|doc| {
                serde_json::json!({
                    "capsule_id": doc.capsule_id.to_string(),
                    "text": doc.text,
                })
            })
            .collect::<Vec<_>>();
        let line = serde_json::json!({
            "benchmark": item.benchmark,
            "query_id": item.query_id,
            "question": item.question,
            "documents": documents,
        });
        out.push_str(&serde_json::to_string(&line)?);
        out.push('\n');
    }
    fs::write(&args.out, out)?;
    eprintln!("exported {} items to {}", items.len(), args.out.display());
    Ok(())
}

fn run_external_run(args: ExternalRunArgs) -> AppResult<()> {
    let identity = RunIdentity::new(&args.input, args.split_file.as_deref(), None)?;
    let items = load_items(
        args.benchmark,
        &args.input,
        args.split_file.as_deref(),
        args.dev_only,
        args.held_out,
        &args.categories,
        args.limit,
    )?;
    let mut rankings = std::collections::HashMap::<String, ExternalRanking>::new();
    for line in fs::read_to_string(&args.rankings)?.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let ranking: ExternalRanking = serde_json::from_str(line)?;
        rankings.insert(ranking.query_id.clone(), ranking);
    }
    let empty = ExternalRanking {
        query_id: String::new(),
        ranked_capsule_ids: Vec::new(),
        build_ns: 0,
        query_ns: 0,
    };
    let mut rows = Vec::with_capacity(items.len());
    for item in &items {
        let ranking = rankings.get(&item.query_id).unwrap_or(&empty);
        let top_k = args.top_k.min(item.documents.len().max(1));
        let mode_run = modes::external_mode_run(
            item,
            &ranking.ranked_capsule_ids,
            top_k,
            ranking.build_ns,
            ranking.query_ns,
        );
        let mut row = modes::finish_run(item, &args.mode_label, top_k, mode_run);
        apply_convomem_abstention(&mut row, item);
        rows.push(row);
    }
    annotate_rows(&mut rows, &identity);
    write_runfile(&args.out, &rows)?;
    eprintln!(
        "scored {} items from {} -> {}",
        rows.len(),
        args.rankings.display(),
        args.out.display()
    );
    Ok(())
}

#[derive(serde::Deserialize)]
struct QueryEmbeddings {
    query_id: String,
    #[serde(default)]
    query_embedding: Vec<f32>,
    #[serde(default)]
    doc_embeddings: std::collections::HashMap<String, Vec<f32>>,
}

fn run_embed_run(args: EmbedRunArgs) -> AppResult<()> {
    let identity = RunIdentity::new(&args.input, args.split_file.as_deref(), None)?;
    let items = load_items(
        args.benchmark,
        &args.input,
        args.split_file.as_deref(),
        args.dev_only,
        args.held_out,
        &args.categories,
        args.limit,
    )?;
    let mut embeddings = std::collections::HashMap::<String, QueryEmbeddings>::new();
    for line in fs::read_to_string(&args.embeddings)?.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parsed: QueryEmbeddings = serde_json::from_str(line)?;
        embeddings.insert(parsed.query_id.clone(), parsed);
    }
    let empty_docs = std::collections::HashMap::new();
    let mut rows = Vec::with_capacity(items.len());
    for item in &items {
        let top_k = args.top_k.min(item.documents.len().max(1));
        let (query_emb, doc_embs) = match embeddings.get(&item.query_id) {
            Some(e) => (e.query_embedding.as_slice(), &e.doc_embeddings),
            None => (&[][..], &empty_docs),
        };
        let mut row = modes::run_item_with_embeddings(
            item,
            top_k,
            query_emb,
            doc_embs,
            args.parent_aggregate,
            &args.mode_label,
        )?;
        apply_convomem_abstention(&mut row, item);
        rows.push(row);
    }
    annotate_rows(&mut rows, &identity);
    write_runfile(&args.out, &rows)?;
    eprintln!(
        "native hybrid: scored {} items (parent_aggregate={}) -> {}",
        rows.len(),
        args.parent_aggregate,
        args.out.display()
    );
    Ok(())
}

fn rrf_fuse(lists: &[&[String]], k_rrf: f64) -> Vec<String> {
    let mut scores = std::collections::HashMap::<String, f64>::new();
    for list in lists {
        for (index, parent) in list.iter().enumerate() {
            *scores.entry(parent.clone()).or_default() += 1.0 / (k_rrf + index as f64 + 1.0);
        }
    }
    let mut ranked = scores.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.cmp(&right.0))
    });
    ranked.into_iter().map(|(parent, _)| parent).collect()
}

fn run_fuse_runs(args: FuseRunsArgs) -> AppResult<()> {
    if args.runs.len() < 2 {
        return Err("fuse-runs requires at least two --runs".into());
    }
    let decoded = args
        .runs
        .iter()
        .map(|path| Ok(decode_jsonl(&fs::read_to_string(path)?)?))
        .collect::<AppResult<Vec<Vec<runfile::RunRow>>>>()?;

    // Index each run by query_id; drive output order from the first run.
    let indexed = decoded
        .iter()
        .map(|rows| {
            rows.iter()
                .map(|row| (row.query_id.clone(), row))
                .collect::<std::collections::HashMap<_, _>>()
        })
        .collect::<Vec<_>>();

    let mut out_rows = Vec::new();
    for base in &decoded[0] {
        let lists = indexed
            .iter()
            .filter_map(|map| map.get(&base.query_id))
            .map(|row| row.returned_parent_ids.as_slice())
            .collect::<Vec<_>>();
        let fused = rrf_fuse(&lists, args.k_rrf)
            .into_iter()
            .take(args.top_k)
            .collect::<Vec<_>>();

        let truth = base
            .ground_truth_ids
            .iter()
            .collect::<std::collections::BTreeSet<_>>();
        let scores = metrics::score_retrieval(&base.ground_truth_ids, &fused);
        let hit_ranks = fused
            .iter()
            .enumerate()
            .filter(|(_, parent)| truth.contains(parent))
            .map(|(index, _)| index + 1)
            .collect::<Vec<_>>();
        let missed = base
            .ground_truth_ids
            .iter()
            .filter(|gt| !fused.contains(gt))
            .cloned()
            .collect::<Vec<_>>();
        // Honest hybrid latency = the sum of the fused retrievers' costs.
        let (build_ns, retrieval_ns) = indexed
            .iter()
            .filter_map(|map| map.get(&base.query_id))
            .fold((0u64, 0u64), |(b, r), row| {
                (
                    b + row.timings.build_or_load_ns,
                    r + row.timings.total_retrieval_ns,
                )
            });

        let mut row = base.clone();
        row.mode = args.mode_label.clone();
        row.top_k = args.top_k;
        row.returned_capsule_ids = fused.clone();
        row.returned_capsule_count = fused.len();
        row.returned_parent_ids = fused;
        row.scores = scores;
        row.hit_ranks = hit_ranks;
        row.missed_ground_truth_ids = missed;
        row.timings.build_or_load_ns = build_ns;
        row.timings.total_retrieval_ns = retrieval_ns;
        row.timings.semantic_score_ns = retrieval_ns;
        row.index_build_time_ns = build_ns;
        out_rows.push(row);
    }
    write_runfile(&args.out, &out_rows)?;
    eprintln!(
        "fused {} runs over {} queries -> {}",
        args.runs.len(),
        out_rows.len(),
        args.out.display()
    );
    Ok(())
}

struct RunIdentity {
    config_hash: String,
    dataset_hash: String,
    split_hash: String,
}

impl RunIdentity {
    fn new(dataset: &Path, split: Option<&Path>, config: Option<&Path>) -> AppResult<Self> {
        Ok(Self {
            config_hash: hash_optional_file(config)?,
            dataset_hash: hash_file_or_dir(dataset)?,
            split_hash: hash_optional_file(split)?,
        })
    }
}

fn annotate_rows(rows: &mut [runfile::RunRow], identity: &RunIdentity) {
    for row in rows {
        row.config_hash = identity.config_hash.clone();
        row.dataset_hash = identity.dataset_hash.clone();
        row.split_hash = identity.split_hash.clone();
    }
}

fn require_frozen_config(path: Option<&Path>) -> AppResult<()> {
    let path = path.ok_or("--held-out requires --config-file with frozen config")?;
    let value: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
    let frozen = value
        .get("frozen")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let label = value
        .get("label")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if frozen && label == "frozen" {
        Ok(())
    } else {
        Err("--held-out requires config JSON with {\"label\":\"frozen\",\"frozen\":true}".into())
    }
}

fn hash_optional_file(path: Option<&Path>) -> AppResult<String> {
    match path {
        Some(path) => hash_file(path),
        None => Ok(String::new()),
    }
}

fn hash_file_or_dir(path: &Path) -> AppResult<String> {
    if path.is_dir() {
        let mut hasher = blake3::Hasher::new();
        let mut files = fs::read_dir(path)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        files.sort();
        for file in files.into_iter().filter(|file| file.is_file()) {
            hasher.update(file.to_string_lossy().as_bytes());
            hasher.update(&fs::read(file)?);
        }
        Ok(hex32(hasher.finalize().as_bytes()))
    } else {
        hash_file(path)
    }
}

fn hash_file(path: &Path) -> AppResult<String> {
    Ok(hex32(blake3::hash(&fs::read(path)?).as_bytes()))
}

fn hex32(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn validate_preset_rows(preset: &ScorecardPreset, rows: &[runfile::RunRow]) -> AppResult<()> {
    match preset {
        ScorecardPreset::Longmemeval => {
            require_benchmark(rows, "LongMemEval")?;
            require_top_k(rows, 10)?;
        }
        ScorecardPreset::Locomo => {
            require_benchmark(rows, "LoCoMo")?;
            require_top_k(rows, 10)?;
            if rows.iter().any(|row| row.retrieves_all_mode) {
                return Err("mempalace-locomo preset forbids retrieves-all rows".into());
            }
        }
        ScorecardPreset::Convomem => {
            require_benchmark(rows, "ConvoMem")?;
            let categories = rows
                .iter()
                .map(|row| row.category.as_str())
                .collect::<std::collections::BTreeSet<_>>();
            for expected in convomem::supported_categories() {
                if !categories.contains(expected) {
                    return Err(format!("missing ConvoMem category for preset: {expected}").into());
                }
            }
        }
    }
    if rows.iter().any(|row| !row.raw_retrieval_only) {
        return Err("scorecard presets require raw retrieval rows".into());
    }
    Ok(())
}

fn require_benchmark(rows: &[runfile::RunRow], benchmark: &str) -> AppResult<()> {
    if rows.iter().all(|row| row.benchmark == benchmark) {
        Ok(())
    } else {
        Err(format!("preset requires benchmark {benchmark}").into())
    }
}

fn require_top_k(rows: &[runfile::RunRow], top_k: usize) -> AppResult<()> {
    if rows.iter().all(|row| row.top_k == top_k) {
        Ok(())
    } else {
        Err(format!("preset requires top-k {top_k}").into())
    }
}

fn run_items(items: &[BenchmarkItem], mode: Mode, top_k: usize) -> AppResult<Vec<runfile::RunRow>> {
    items
        .iter()
        .map(|item| modes::run_item(item, mode, top_k.min(item.documents.len())))
        .collect()
}

fn write_runfile(path: &Path, rows: &[runfile::RunRow]) -> AppResult<()> {
    write_parent(path)?;
    fs::write(path, encode_jsonl(rows)?)?;
    Ok(())
}

fn write_split(path: &Path, items: &[BenchmarkItem], dev_size: usize, seed: u64) -> AppResult<()> {
    let ids = items
        .iter()
        .map(|item| item.query_id.clone())
        .collect::<Vec<_>>();
    let split = create_split(&ids, dev_size, seed);
    write_parent(path)?;
    fs::write(path, serde_json::to_string_pretty(&split)?)?;
    Ok(())
}

fn read_split(path: &Path) -> AppResult<SplitFile> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn write_parent(path: &Path) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}
