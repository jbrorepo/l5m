#![forbid(unsafe_code)]
//! L5M MCP server binary — stdio transport.
//!
//! Configuration via environment (set by the MCP host, e.g. Claude Desktop):
//!   L5M_DATA_DIR     durable data directory (default: ./l5m_data).
//!                    Set to "memory" for an ephemeral, non-durable store.
//!   L5M_TENANT       tenant id this connection is bound to (default 1)
//!   L5M_CONTEXT      context mask, hex (default 0xffff)
//!   L5M_POLICY       policy mask, hex (default 0xffff)
//!   L5M_TRUST_FLOOR  minimum trust recalled memories must meet (default 0)
//!
//! Protocol I/O is newline-delimited JSON-RPC on stdin/stdout; diagnostics go
//! to stderr only (stdout is reserved for the protocol).

use std::io::{BufRead, Write};

use l5m_mcp::{McpServer, Principal};

fn main() {
    let principal = match Principal::from_env() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("l5m-mcp: bad configuration: {e}");
            std::process::exit(2);
        }
    };
    let data_dir = std::env::var("L5M_DATA_DIR").unwrap_or_else(|_| "./l5m_data".into());

    let mut server = if data_dir == "memory" {
        eprintln!(
            "l5m-mcp: EPHEMERAL store (no durability), tenant {}",
            principal.tenant_id
        );
        McpServer::new(l5m_core::MemoryStore::empty(), principal)
    } else {
        match McpServer::open_durable(&data_dir, principal.clone()) {
            Ok(server) => {
                eprintln!(
                    "l5m-mcp: durable store at {data_dir}, tenant {}",
                    principal.tenant_id
                );
                server
            }
            Err(e) => {
                eprintln!("l5m-mcp: failed to open store at {data_dir}: {e}");
                std::process::exit(1);
            }
        }
    };

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => break, // host closed the pipe
        };
        if let Some(response) = server.handle_line(&line) {
            if writeln!(out, "{response}")
                .and_then(|()| out.flush())
                .is_err()
            {
                break;
            }
        }
    }
    eprintln!("l5m-mcp: shutting down");
}
