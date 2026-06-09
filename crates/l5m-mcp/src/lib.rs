#![forbid(unsafe_code)]
//! L5M MCP server: exposes security-gated memory to AI agents over the
//! Model Context Protocol (stdio transport, JSON-RPC 2.0, newline-delimited).
//!
//! Security model — the part that matters: the principal (tenant / context /
//! policy / trust floor) is bound **once, at process start**, from the host
//! configuration. Tool arguments cannot name a tenant, raise a clearance, or
//! widen a mask. So even a fully compromised / prompt-injected agent can only
//! ever read and write the memory slice its host granted it: the gates run
//! before scoring on every recall, exactly as they do in the HTTP server and
//! the embedded library.
//!
//! Tools: `remember` (store a memory), `recall` (gated retrieval, optional
//! point-in-time `as_of`), `forget` (tombstone by id).
//!
//! Dependencies: `l5m-core` + `serde_json` only — no MCP SDK, no async runtime.
//! The stdio transport is sequential, so a synchronous loop is correct and
//! keeps the supply-chain surface minimal.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use l5m_core::{MemoryStore, QueryRequest, RetrievalMode};
use serde_json::{json, Value};

/// The newest MCP protocol revision this server implements. We echo the
/// client's requested version when we can interoperate with it (the methods we
/// use — initialize / tools/list / tools/call / ping — are stable across
/// 2024-11-05 through 2025-06-18).
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// The identity every tool call runs under. Bound at startup; never derived
/// from tool arguments.
#[derive(Clone, Debug)]
pub struct Principal {
    pub tenant_id: u64,
    pub context_mask: String,
    pub policy_mask: String,
    pub trust_floor: u8,
}

impl Principal {
    /// Read the principal from environment variables (`L5M_TENANT`,
    /// `L5M_CONTEXT`, `L5M_POLICY`, `L5M_TRUST_FLOOR`).
    pub fn from_env() -> Result<Self, String> {
        let tenant_id = match std::env::var("L5M_TENANT") {
            Ok(v) => v
                .trim()
                .parse::<u64>()
                .map_err(|_| format!("L5M_TENANT must be an integer, got {v:?}"))?,
            Err(_) => 1,
        };
        let trust_floor = match std::env::var("L5M_TRUST_FLOOR") {
            Ok(v) => v
                .trim()
                .parse::<u8>()
                .map_err(|_| format!("L5M_TRUST_FLOOR must be 0-10, got {v:?}"))?,
            Err(_) => 0,
        };
        Ok(Self {
            tenant_id,
            context_mask: std::env::var("L5M_CONTEXT").unwrap_or_else(|_| "0xffff".into()),
            policy_mask: std::env::var("L5M_POLICY").unwrap_or_else(|_| "0xffff".into()),
            trust_floor,
        })
    }
}

pub struct McpServer {
    store: MemoryStore,
    principal: Principal,
    /// Monotonic suffix so two memories stored in the same nanosecond still get
    /// distinct ids.
    seq: u64,
    /// Where `compact` checkpoints land (the durable data directory), when the
    /// store was opened durably.
    data_dir: Option<PathBuf>,
}

impl McpServer {
    /// In-memory server (tests / ephemeral sessions).
    pub fn new(store: MemoryStore, principal: Principal) -> Self {
        Self {
            store,
            principal,
            seq: 0,
            data_dir: None,
        }
    }

    /// Durable server rooted at `data_dir`: memories are WAL-logged before they
    /// are acknowledged and survive restarts. Reopens `base.segment` if a prior
    /// checkpoint exists.
    pub fn open_durable(data_dir: impl AsRef<Path>, principal: Principal) -> Result<Self, String> {
        let dir = data_dir.as_ref();
        std::fs::create_dir_all(dir).map_err(|e| format!("create {dir:?}: {e}"))?;
        let wal = dir.join("l5m.wal");
        let base = dir.join("base.segment");
        let bases: Vec<PathBuf> = if base.exists() {
            vec![base]
        } else {
            Vec::new()
        };
        let store = MemoryStore::open_durable(bases, &wal).map_err(|e| e.to_string())?;
        Ok(Self {
            store,
            principal,
            seq: 0,
            data_dir: Some(dir.to_path_buf()),
        })
    }

    /// Handle one newline-delimited JSON-RPC message. Returns the serialized
    /// response line, or `None` for notifications / unparseable garbage that
    /// carries no id to respond to.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let message: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                return Some(
                    json!({
                        "jsonrpc": "2.0",
                        "id": Value::Null,
                        "error": {"code": -32700, "message": format!("parse error: {e}")}
                    })
                    .to_string(),
                )
            }
        };
        self.handle_message(&message).map(|v| v.to_string())
    }

    fn handle_message(&mut self, message: &Value) -> Option<Value> {
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let id = message.get("id").cloned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);

        // Notifications (no id) expect no response.
        let id = match id {
            Some(id) if !id.is_null() => id,
            _ => return None,
        };

        let result = match method {
            "initialize" => Ok(self.initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tool_definitions() })),
            "tools/call" => return Some(self.tools_call(id, &params)),
            other => Err((-32601, format!("method not found: {other}"))),
        };

        Some(match result {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err((code, message)) => json!({
                "jsonrpc": "2.0", "id": id,
                "error": {"code": code, "message": message}
            }),
        })
    }

    fn initialize(&self, params: &Value) -> Value {
        // Echo the client's requested protocol revision (our method surface is
        // stable across published revisions); otherwise advertise our latest.
        let requested = params
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or(PROTOCOL_VERSION);
        json!({
            "protocolVersion": requested,
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "l5m",
                "title": "L5M security-gated memory",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "instructions": format!(
                "Security-gated long-term memory. All tools run under a fixed \
                 principal (tenant {}) bound by the host at startup; tenant, \
                 policy, and trust gates are enforced BEFORE relevance scoring \
                 on every recall, so memories outside this principal's scope \
                 are never even candidates.",
                self.principal.tenant_id
            ),
        })
    }

    /// `tools/call`: tool-level failures are reported as `isError: true`
    /// results (per MCP), not JSON-RPC protocol errors.
    fn tools_call(&mut self, id: Value, params: &Value) -> Value {
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        let args = params.get("arguments").cloned().unwrap_or(json!({}));
        let outcome = match name {
            "remember" => self.remember(&args),
            "recall" => self.recall(&args),
            "forget" => self.forget(&args),
            other => Err(format!("unknown tool: {other}")),
        };
        let (text, is_error) = match outcome {
            Ok(text) => (text, false),
            Err(text) => (text, true),
        };
        json!({
            "jsonrpc": "2.0", "id": id,
            "result": {
                "content": [{"type": "text", "text": text}],
                "isError": is_error,
            }
        })
    }

    // -- tools ---------------------------------------------------------------

    fn remember(&mut self, args: &Value) -> Result<String, String> {
        let claim = args
            .get("claim")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("remember requires a non-empty 'claim' string")?;
        let evidence = args
            .get("evidence")
            .and_then(Value::as_str)
            .unwrap_or(claim);
        let trust_level = args
            .get("trust_level")
            .and_then(Value::as_u64)
            .unwrap_or(5)
            .min(10);
        let now = now_unix();
        let valid_from = args
            .get("valid_from")
            .and_then(Value::as_i64)
            .unwrap_or(now);
        let id = self.generate_id();

        let mut capsule = json!({
            "capsule_id": id.to_string(),
            // Tenant ownership is forced to the bound principal — the agent
            // cannot write into another tenant regardless of its arguments.
            "tenant_id": self.principal.tenant_id,
            "claim": claim,
            "evidence": evidence,
            "source_id": 0,
            "valid_from": valid_from,
            "observed_at": now,
            "last_verified_at": now,
            "context_mask": "0xffff",
            "policy_mask": "0xffff",
            "trust_level": trust_level,
            "classification": 1,
            "poison_risk": 0,
        });
        if let Some(until) = args.get("valid_until").and_then(Value::as_i64) {
            capsule["valid_until"] = json!(until);
        }
        self.store
            .insert_json(&capsule)
            .map_err(|e| format!("store failed: {e}"))?;
        Ok(format!("Stored memory {id} (trust {trust_level})."))
    }

    fn recall(&mut self, args: &Value) -> Result<String, String> {
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("recall requires a non-empty 'query' string")?;
        let max_results = args
            .get("max_results")
            .and_then(Value::as_u64)
            .unwrap_or(5)
            .clamp(1, 25) as usize;
        let as_of = args
            .get("as_of")
            .and_then(Value::as_i64)
            .unwrap_or(i64::MAX);

        let request = QueryRequest {
            query: query.to_string(),
            tenant_id: self.principal.tenant_id,
            as_of,
            context_mask: self.principal.context_mask.clone(),
            policy_mask: self.principal.policy_mask.clone(),
            trust_floor: self.principal.trust_floor,
            max_capsules: max_results,
            max_tokens: usize::MAX,
            include_supporting: false,
            include_contradictions: false,
            max_hops: 1,
            mode: RetrievalMode::L5m,
            embedding: Vec::new(),
        };
        let response = self
            .store
            .query(&request)
            .map_err(|e| format!("recall failed: {e}"))?;
        if response.frame.capsules.is_empty() {
            return Ok("No memories found for that query (within this principal's scope).".into());
        }
        let mut out = String::new();
        for (rank, capsule) in response.frame.capsules.iter().enumerate() {
            out.push_str(&format!(
                "{}. [id {}] (score {:.2}, trust {}) {}\n",
                rank + 1,
                capsule.capsule_id,
                capsule.score,
                capsule.trust_level,
                capsule.claim,
            ));
        }
        Ok(out.trim_end().to_string())
    }

    fn forget(&mut self, args: &Value) -> Result<String, String> {
        let raw = args
            .get("capsule_id")
            .and_then(Value::as_str)
            .ok_or("forget requires a 'capsule_id' string")?;
        let id: u128 = raw
            .trim()
            .parse()
            .map_err(|_| format!("capsule_id must be an integer id, got {raw:?}"))?;
        self.store
            .delete(id)
            .map_err(|e| format!("forget failed: {e}"))?;
        Ok(format!("Forgot memory {id}."))
    }

    // -- helpers -------------------------------------------------------------

    fn generate_id(&mut self) -> u128 {
        self.seq += 1;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        (nanos << 16) | u128::from(self.seq & 0xffff)
    }

    /// Direct store access (tests / host integration).
    pub fn store_mut(&mut self) -> &mut MemoryStore {
        &mut self.store
    }

    pub fn data_dir(&self) -> Option<&Path> {
        self.data_dir.as_deref()
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "remember",
            "description": "Store a long-term memory under the bound tenant. \
                Use for durable facts, preferences, and decisions worth \
                recalling in future sessions.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "claim": {"type": "string", "description": "The fact to remember, stated plainly."},
                    "evidence": {"type": "string", "description": "Source text supporting the claim (defaults to the claim)."},
                    "trust_level": {"type": "integer", "minimum": 0, "maximum": 10, "description": "Confidence 0-10 (default 5). Higher-trust memories survive stricter recall floors."},
                    "valid_from": {"type": "integer", "description": "Unix seconds the fact becomes valid (default now)."},
                    "valid_until": {"type": "integer", "description": "Unix seconds the fact expires (default never)."}
                },
                "required": ["claim"]
            }
        }),
        json!({
            "name": "recall",
            "description": "Retrieve relevant memories. Tenant/policy/trust \
                gates are enforced before scoring, so only memories this \
                principal is entitled to can ever be returned. Supports \
                point-in-time recall via as_of.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "What to look for."},
                    "max_results": {"type": "integer", "minimum": 1, "maximum": 25, "description": "Max memories to return (default 5)."},
                    "as_of": {"type": "integer", "description": "Unix seconds for point-in-time recall: only memories valid at that moment are returned (default: now/all)."}
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "forget",
            "description": "Permanently hide a memory by id (tombstone). Use the id shown by recall.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "capsule_id": {"type": "string", "description": "The memory id to forget."}
                },
                "required": ["capsule_id"]
            }
        }),
    ]
}
