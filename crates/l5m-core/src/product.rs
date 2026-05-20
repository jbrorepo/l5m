use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{compile_segment, CompileOptions, Result};

#[derive(Clone, Debug)]
pub struct ProductCompileOptions {
    pub input_jsonl: PathBuf,
    pub output_dir: PathBuf,
    pub epoch: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProductCompileManifest {
    pub input_path: PathBuf,
    pub output_dir: PathBuf,
    pub epoch: u64,
    pub views: Vec<ProductViewManifest>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProductViewManifest {
    pub view: String,
    pub segment_path: PathBuf,
    pub capsule_count: usize,
}

#[derive(Deserialize)]
struct ProductMemoryRecord {
    memory_id: String,
    tenant_id: u64,
    session_id: String,
    #[serde(default)]
    evidence_id: Option<String>,
    observed_at: i64,
    valid_from: i64,
    #[serde(default)]
    valid_until: Option<i64>,
    context_mask: String,
    policy_mask: String,
    trust_level: u8,
    classification: u8,
    poison_risk: u8,
    turns: Vec<ProductTurn>,
}

#[derive(Deserialize)]
struct ProductTurn {
    turn_id: String,
    role: String,
    text: String,
}

#[derive(Serialize)]
struct RawProductCapsule {
    capsule_id: String,
    tenant_id: u64,
    claim: String,
    evidence: String,
    source_id: u64,
    source_uri: String,
    valid_from: i64,
    valid_until: Option<i64>,
    observed_at: i64,
    last_verified_at: i64,
    context_mask: String,
    policy_mask: String,
    trust_level: u8,
    classification: u8,
    poison_risk: u8,
}

pub fn compile_product_memories(options: ProductCompileOptions) -> Result<ProductCompileManifest> {
    fs::create_dir_all(&options.output_dir)?;
    let records = read_product_records(&options.input_jsonl)?;
    let mut views = Vec::new();
    for view in [
        "session",
        "user-turn",
        "assistant-turn",
        "turn",
        "native-capsule",
    ] {
        let capsules = build_view_capsules(&records, view);
        let json_path = options.output_dir.join(format!("{view}.json"));
        let segment_path = options.output_dir.join(format!("{view}.segment"));
        fs::write(&json_path, serde_json::to_string_pretty(&capsules)?)?;
        compile_segment(CompileOptions {
            input_json: json_path,
            output_segment: segment_path.clone(),
            epoch: options.epoch,
        })?;
        views.push(ProductViewManifest {
            view: view.to_string(),
            segment_path,
            capsule_count: capsules.len(),
        });
    }
    let manifest = ProductCompileManifest {
        input_path: options.input_jsonl,
        output_dir: options.output_dir,
        epoch: options.epoch,
        views,
    };
    fs::write(
        manifest.output_dir.join("product-manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;
    Ok(manifest)
}

pub fn segment_paths_from_product_dir(path: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
    let manifest_path = path.as_ref().join("product-manifest.json");
    let manifest: ProductCompileManifest = serde_json::from_slice(&fs::read(manifest_path)?)?;
    Ok(manifest
        .views
        .into_iter()
        .map(|view| view.segment_path)
        .collect())
}

fn read_product_records(path: &Path) -> Result<Vec<ProductMemoryRecord>> {
    let text = fs::read_to_string(path)?;
    let mut records = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let record = serde_json::from_str(trimmed).map_err(|err| {
            crate::L5mError::Format(format!(
                "invalid product memory JSONL line {}: {err}",
                index + 1
            ))
        })?;
        records.push(record);
    }
    Ok(records)
}

fn build_view_capsules(records: &[ProductMemoryRecord], view: &str) -> Vec<RawProductCapsule> {
    let mut capsules = Vec::new();
    for record in records {
        match view {
            "session" => capsules.push(record_to_capsule(record, view, None, session_text(record))),
            "user-turn" => capsules.extend(
                record
                    .turns
                    .iter()
                    .filter(|turn| turn.role == "user")
                    .map(|turn| record_to_capsule(record, view, Some(turn), turn.text.clone())),
            ),
            "assistant-turn" => capsules.extend(
                record
                    .turns
                    .iter()
                    .filter(|turn| turn.role == "assistant")
                    .map(|turn| record_to_capsule(record, view, Some(turn), turn.text.clone())),
            ),
            "turn" | "native-capsule" => capsules.extend(
                record
                    .turns
                    .iter()
                    .map(|turn| record_to_capsule(record, view, Some(turn), turn.text.clone())),
            ),
            _ => {}
        }
    }
    capsules
}

fn record_to_capsule(
    record: &ProductMemoryRecord,
    view: &str,
    turn: Option<&ProductTurn>,
    text: String,
) -> RawProductCapsule {
    let turn_id = turn.map(|turn| turn.turn_id.as_str()).unwrap_or("session");
    let material = format!(
        "{}:{}:{}:{}",
        record.memory_id, record.session_id, turn_id, view
    );
    let source_uri = format!(
        "l5m://product?memory_id={}&session_id={}&turn_id={}&evidence_id={}&view={}",
        record.memory_id,
        record.session_id,
        turn_id,
        record.evidence_id.as_deref().unwrap_or(""),
        view
    );
    RawProductCapsule {
        capsule_id: stable_u128(&material).to_string(),
        tenant_id: record.tenant_id,
        claim: text,
        evidence: source_uri.clone(),
        source_id: stable_u64(&record.memory_id),
        source_uri,
        valid_from: record.valid_from,
        valid_until: record.valid_until,
        observed_at: record.observed_at,
        last_verified_at: record.observed_at,
        context_mask: record.context_mask.clone(),
        policy_mask: record.policy_mask.clone(),
        trust_level: record.trust_level,
        classification: record.classification,
        poison_risk: record.poison_risk,
    }
}

fn session_text(record: &ProductMemoryRecord) -> String {
    record
        .turns
        .iter()
        .map(|turn| format!("{}: {}", turn.role, turn.text))
        .collect::<Vec<_>>()
        .join("\n")
}

fn stable_u128(value: &str) -> u128 {
    let hash = blake3::hash(value.as_bytes());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&hash.as_bytes()[0..8]);
    u64::from_le_bytes(bytes) as u128
}

fn stable_u64(value: &str) -> u64 {
    let hash = blake3::hash(value.as_bytes());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&hash.as_bytes()[0..8]);
    u64::from_le_bytes(bytes)
}
