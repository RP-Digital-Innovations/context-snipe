//! Hand-rolled JSON-RPC 2.0 + Model Context Protocol server over stdio.
//!
//! Transport: newline-delimited JSON-RPC messages on stdin/stdout (the MCP
//! stdio transport). stdout is the protocol channel and carries *only* JSON-RPC
//! — every diagnostic goes to stderr. We implement the slice of MCP a tools
//! server needs: initialize, tools/list, tools/call, ping.

use std::io::{self, BufRead, Write};

use serde_json::{json, Value};

/// Protocol revision we advertise when the client doesn't pin one.
const PROTOCOL_VERSION: &str = "2025-06-18";

pub fn serve() -> Result<(), String> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    eprintln!(
        "[context-snipe] MCP server v{} ready on stdio",
        env!("CARGO_PKG_VERSION")
    );

    for line in stdin.lock().lines() {
        let line = line.map_err(|e| format!("stdin read error: {e}"))?;
        // Tolerate a leading UTF-8 BOM (some clients/shells prepend one); note
        // that str::trim() does not treat U+FEFF as whitespace.
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() {
            continue;
        }

        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[context-snipe] parse error: {e}");
                send_error(&mut out, Value::Null, -32700, "Parse error")?;
                continue;
            }
        };

        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);

        // No id => notification: act on it, never reply.
        let Some(id) = id else {
            if method == "notifications/initialized" {
                eprintln!("[context-snipe] client initialized");
            } else {
                eprintln!("[context-snipe] ignoring notification: {method}");
            }
            continue;
        };

        match method {
            "initialize" => {
                // Echo the client's protocol version when present; else our default.
                let proto = params
                    .get("protocolVersion")
                    .and_then(|v| v.as_str())
                    .unwrap_or(PROTOCOL_VERSION);
                send_result(
                    &mut out,
                    id,
                    json!({
                        "protocolVersion": proto,
                        "capabilities": { "tools": { "listChanged": false } },
                        "serverInfo": {
                            "name": "context-snipe",
                            "version": env!("CARGO_PKG_VERSION")
                        }
                    }),
                )?;
            }
            "ping" => send_result(&mut out, id, json!({}))?,
            "tools/list" => send_result(&mut out, id, json!({ "tools": tool_defs() }))?,
            "tools/call" => {
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                // Per MCP, tool failures are reported in the result with
                // isError:true (not as a JSON-RPC protocol error) so the model
                // can read and react to them.
                let (text, is_error) = match call_tool(name, &args) {
                    Ok(text) => (text, false),
                    Err(e) => (format!("Error: {e}"), true),
                };
                send_result(
                    &mut out,
                    id,
                    json!({
                        "content": [ { "type": "text", "text": text } ],
                        "isError": is_error
                    }),
                )?;
            }
            other => send_error(&mut out, id, -32601, &format!("Method not found: {other}"))?,
        }
    }
    Ok(())
}

fn tool_defs() -> Value {
    json!([
        {
            "name": "scan_dependencies",
            "description": "List a project's resolved dependencies by parsing its lockfiles/manifests (Cargo.lock, package-lock.json, package.json, requirements.txt, go.mod/go.sum). Returns each dependency's name, version, and ecosystem. Use this to give the assistant ground-truth about exactly which packages and versions a project actually uses.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the project directory. Defaults to the current working directory."
                    }
                }
            }
        },
        {
            "name": "check_vulnerabilities",
            "description": "Cross-reference a project's resolved dependencies against the OSV.dev vulnerability database and report which packages have known advisories (CVE/GHSA/RUSTSEC/PYSEC), with computed severity. Only reports advisories for packages actually present in the dependency tree — not generic noise. Requires network access to api.osv.dev.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the project directory. Defaults to the current working directory."
                    },
                    "severity_min": {
                        "type": "string",
                        "enum": ["low", "medium", "high", "critical"],
                        "description": "Optional. Only report advisories at or above this severity."
                    }
                }
            }
        }
    ])
}

fn call_tool(name: &str, args: &Value) -> Result<String, String> {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    match name {
        "scan_dependencies" => crate::scan::list_dependencies(path),
        "check_vulnerabilities" => {
            let min = args.get("severity_min").and_then(|v| v.as_str());
            crate::scan::check(path, min)
        }
        other => Err(format!("unknown tool: {other}")),
    }
}

fn send_result(out: &mut impl Write, id: Value, result: Value) -> Result<(), String> {
    write_msg(out, &json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn send_error(out: &mut impl Write, id: Value, code: i64, message: &str) -> Result<(), String> {
    write_msg(
        out,
        &json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
    )
}

fn write_msg(out: &mut impl Write, msg: &Value) -> Result<(), String> {
    let s = serde_json::to_string(msg).map_err(|e| format!("serialize: {e}"))?;
    out.write_all(s.as_bytes())
        .and_then(|_| out.write_all(b"\n"))
        .and_then(|_| out.flush())
        .map_err(|e| format!("stdout write: {e}"))
}
