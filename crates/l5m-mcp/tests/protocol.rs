// MCP protocol + security tests. Drives the server through the same
// newline-delimited JSON-RPC surface an MCP host uses, with special attention
// to the property that matters: the principal is bound at startup and tool
// arguments can never widen it.

use l5m_core::MemoryStore;
use l5m_mcp::{McpServer, Principal};
use serde_json::{json, Value};

fn principal(tenant: u64) -> Principal {
    Principal {
        tenant_id: tenant,
        context_mask: "0xffff".into(),
        policy_mask: "0xffff".into(),
        trust_floor: 0,
    }
}

fn server(tenant: u64) -> McpServer {
    McpServer::new(MemoryStore::empty(), principal(tenant))
}

fn rpc(server: &mut McpServer, id: u64, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string();
    let response = server.handle_line(&line).expect("expected a response");
    serde_json::from_str(&response).unwrap()
}

fn call_tool(server: &mut McpServer, name: &str, args: Value) -> (String, bool) {
    let resp = rpc(
        server,
        42,
        "tools/call",
        json!({"name": name, "arguments": args}),
    );
    let result = &resp["result"];
    let text = result["content"][0]["text"].as_str().unwrap().to_string();
    (text, result["isError"].as_bool().unwrap_or(false))
}

#[test]
fn initialize_handshake_and_tools_list() {
    let mut s = server(1);
    let resp = rpc(
        &mut s,
        1,
        "initialize",
        json!({"protocolVersion": "2025-03-26", "capabilities": {}}),
    );
    assert_eq!(
        resp["result"]["protocolVersion"], "2025-03-26",
        "echoes client revision"
    );
    assert_eq!(resp["result"]["serverInfo"]["name"], "l5m");
    assert!(resp["result"]["capabilities"]["tools"].is_object());

    // notifications/initialized expects NO response.
    assert!(s
        .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
        .is_none());

    let resp = rpc(&mut s, 2, "tools/list", json!({}));
    let tools: Vec<&str> = resp["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(tools, vec!["remember", "recall", "forget"]);
}

#[test]
fn remember_recall_forget_round_trip() {
    let mut s = server(1);

    let (text, is_err) = call_tool(
        &mut s,
        "remember",
        json!({"claim": "the deploy password is opal-kelpstone", "trust_level": 9}),
    );
    assert!(!is_err, "{text}");
    let id = text
        .split_whitespace()
        .nth(2)
        .expect("id in confirmation")
        .to_string();

    let (text, is_err) = call_tool(&mut s, "recall", json!({"query": "deploy password"}));
    assert!(!is_err);
    assert!(text.contains("opal-kelpstone"), "recall finds it: {text}");
    assert!(text.contains(&id), "recall shows the id");

    let (text, is_err) = call_tool(&mut s, "forget", json!({"capsule_id": id}));
    assert!(!is_err, "{text}");

    let (text, _) = call_tool(&mut s, "recall", json!({"query": "deploy password"}));
    assert!(
        !text.contains("opal-kelpstone"),
        "forgotten memory must not resurface: {text}"
    );
}

#[test]
fn tenant_isolation_holds_over_mcp() {
    // Tenant 7's secret is in the store; this MCP connection is bound to
    // tenant 42. A perfect-match recall must come back empty.
    let mut s = server(42);
    s.store_mut()
        .insert_json(&json!({
            "capsule_id": "555", "tenant_id": 7,
            "claim": "tenant seven secret: vermilion-kelpstone",
            "evidence": "tenant seven secret: vermilion-kelpstone",
            "source_id": 1, "valid_from": 1, "observed_at": 1, "last_verified_at": 1,
            "context_mask": "0xffff", "policy_mask": "0xffff",
            "trust_level": 10, "classification": 1, "poison_risk": 0
        }))
        .unwrap();

    let (text, is_err) = call_tool(
        &mut s,
        "recall",
        json!({"query": "tenant seven secret: vermilion-kelpstone"}),
    );
    assert!(!is_err);
    assert!(
        !text.contains("vermilion"),
        "cross-tenant memory must never be returned: {text}"
    );
}

#[test]
fn tool_arguments_cannot_override_the_bound_principal() {
    // A hostile/confused agent passes tenant-ish and scope-ish arguments to
    // every tool. They must be ignored: writes land in the bound tenant and
    // reads stay inside it.
    let mut s = server(42);
    let (_, is_err) = call_tool(
        &mut s,
        "remember",
        json!({
            "claim": "planted fact amaranth-kelpstone",
            // None of these are real parameters; a correct server ignores them.
            "tenant_id": 7, "tenant": 7, "policy_mask": "0x0", "trust_floor": 10
        }),
    );
    assert!(!is_err);

    // Visible to the bound tenant (42)…
    let (text, _) = call_tool(&mut s, "recall", json!({"query": "amaranth-kelpstone"}));
    assert!(text.contains("amaranth"), "write landed in bound tenant");

    // …and NOT visible to tenant 7, proving the write was not redirected.
    let resp = s
        .store_mut()
        .query(&l5m_core::QueryRequest {
            query: "amaranth-kelpstone".into(),
            tenant_id: 7,
            as_of: i64::MAX,
            context_mask: "0xffff".into(),
            policy_mask: "0xffff".into(),
            trust_floor: 0,
            max_capsules: 8,
            max_tokens: usize::MAX,
            include_supporting: false,
            include_contradictions: false,
            max_hops: 1,
            mode: l5m_core::RetrievalMode::L5m,
            embedding: Vec::new(),
        })
        .unwrap();
    assert!(
        resp.frame
            .capsules
            .iter()
            .all(|c| !c.claim.contains("amaranth")),
        "tenant 7 must not see the write tenant 42 made"
    );
}

#[test]
fn point_in_time_recall_respects_as_of() {
    let mut s = server(1);
    let (_, e1) = call_tool(
        &mut s,
        "remember",
        json!({"claim": "office moved to amber street", "valid_from": 1000}),
    );
    let (_, e2) = call_tool(
        &mut s,
        "remember",
        json!({"claim": "office moved to cobalt avenue", "valid_from": 5000}),
    );
    assert!(!e1 && !e2);

    // As of t=2000, only the amber-street fact is valid.
    let (text, _) = call_tool(
        &mut s,
        "recall",
        json!({"query": "office moved", "as_of": 2000}),
    );
    assert!(text.contains("amber street"), "{text}");
    assert!(
        !text.contains("cobalt avenue"),
        "future fact excluded: {text}"
    );
}

#[test]
fn errors_are_tool_results_not_protocol_errors() {
    let mut s = server(1);
    // Missing required argument -> isError result, not a JSON-RPC error.
    let resp = rpc(
        &mut s,
        9,
        "tools/call",
        json!({"name": "remember", "arguments": {}}),
    );
    assert!(
        resp.get("error").is_none(),
        "tool failure is not a protocol error"
    );
    assert_eq!(resp["result"]["isError"], true);

    // Unknown method -> -32601 protocol error.
    let resp = rpc(&mut s, 10, "no/such/method", json!({}));
    assert_eq!(resp["error"]["code"], -32601);

    // Garbage line -> -32700 parse error.
    let raw = s.handle_line("{not json").unwrap();
    let parsed: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(parsed["error"]["code"], -32700);
}

#[test]
fn durable_store_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut s = McpServer::open_durable(dir.path(), principal(1)).unwrap();
        let (text, is_err) = call_tool(
            &mut s,
            "remember",
            json!({"claim": "durable fact saffron-kelpstone"}),
        );
        assert!(!is_err, "{text}");
    } // process "exit"

    let mut s = McpServer::open_durable(dir.path(), principal(1)).unwrap();
    let (text, _) = call_tool(&mut s, "recall", json!({"query": "saffron-kelpstone"}));
    assert!(text.contains("saffron"), "memory survived restart: {text}");
}
