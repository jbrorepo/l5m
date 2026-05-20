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
        if item.abstention {
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
    }
    annotate_rows(&mut rows, &identity);
    write_runfile(&args.out, &rows)?;
    Ok(())
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
