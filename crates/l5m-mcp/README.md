# l5m-mcp — security-gated memory for AI agents over MCP

An [MCP](https://modelcontextprotocol.io) server that gives any MCP host
(Claude Desktop, Claude Code, ChatGPT desktop, Copilot, custom agents) durable,
**security-gated** long-term memory backed by the L5M engine.

## Why this is different from other memory MCP servers

The principal — tenant, context mask, policy mask, trust floor — is bound
**once, at process start, by the host configuration**. Tool arguments cannot
name a tenant, raise a clearance, or widen a mask. L5M then enforces every gate
**before relevance scoring**, so a memory outside the bound principal's scope
is never even a retrieval candidate.

Practical consequence: even a fully prompt-injected agent can only read and
write the memory slice its host granted it. Other memory servers trust the
agent to pass the right user/scope per call; this one structurally can't be
talked out of its scope.

## Tools

| Tool | What it does |
|---|---|
| `remember` | Store a fact (`claim`, optional `evidence`, `trust_level` 0-10, `valid_from`/`valid_until`) |
| `recall` | Gated retrieval (`query`, `max_results`, optional `as_of` for point-in-time recall) |
| `forget` | Tombstone a memory by id |

`recall` supports **time-travel**: pass `as_of` (unix seconds) and only
memories valid at that moment are returned — useful for "what did we believe
last quarter?" questions.

## Install & configure

```bash
cargo install --path crates/l5m-mcp   # or grab a signed release binary
```

Claude Desktop (`claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "l5m-memory": {
      "command": "l5m-mcp",
      "env": {
        "L5M_DATA_DIR": "/Users/you/.l5m",
        "L5M_TENANT": "1",
        "L5M_TRUST_FLOOR": "0"
      }
    }
  }
}
```

Claude Code:

```bash
claude mcp add l5m-memory -e L5M_DATA_DIR=~/.l5m -- l5m-mcp
```

## Configuration

| Env var | Default | Meaning |
|---|---|---|
| `L5M_DATA_DIR` | `./l5m_data` | Durable store location (WAL-backed; survives restarts). `memory` = ephemeral |
| `L5M_TENANT` | `1` | Tenant this connection is bound to |
| `L5M_CONTEXT` | `0xffff` | Context mask the principal queries under (hex) |
| `L5M_POLICY` | `0xffff` | Policy/clearance mask (hex) |
| `L5M_TRUST_FLOOR` | `0` | Minimum trust level recalled memories must meet |

Multi-agent isolation: run one `l5m-mcp` entry per agent/user with different
`L5M_TENANT` values against the same `L5M_DATA_DIR` — the tenant gate keeps
their memories mutually invisible, enforced in the engine rather than by
convention.

## Durability

Writes are appended to a write-ahead log before they are acknowledged and are
replayed on restart, so an acknowledged `remember` survives a crash. Storage
uses the same gate/index/retrieval code path as the L5M HTTP server and the
embedded library.

## Protocol notes

stdio transport, newline-delimited JSON-RPC 2.0. Implements `initialize`,
`tools/list`, `tools/call`, `ping`; diagnostics go to stderr only. Tool
failures are returned as `isError: true` tool results (per spec), not protocol
errors. Tested against revisions 2024-11-05 → 2025-06-18.
