//! context-snipe — deterministic dependency + CVE context for AI coding tools.
//!
//! Two modes from one binary:
//!   * `serve` (default) — speak MCP over stdio for Claude Desktop, Cursor, etc.
//!   * `scan` / `deps`   — run the same logic from a terminal, no client needed.

mod deps;
mod http;
mod mcp;
mod osv;
mod scan;

use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("serve");

    match mode {
        "serve" => match mcp::serve() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("context-snipe: fatal: {e}");
                ExitCode::FAILURE
            }
        },
        "scan" => run(scan::check(args.get(2).map(String::as_str).unwrap_or("."), None)),
        "deps" => run(scan::list_dependencies(
            args.get(2).map(String::as_str).unwrap_or("."),
        )),
        "--version" | "-V" | "version" => {
            println!("context-snipe {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        "--help" | "-h" | "help" => {
            print_help();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("context-snipe: unknown command '{other}'\n");
            print_help();
            ExitCode::FAILURE
        }
    }
}

fn run(result: Result<String, String>) -> ExitCode {
    match result {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("context-snipe: {e}");
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    print!(
        "context-snipe {ver}\n\
Deterministic dependency + CVE context for AI coding tools, over MCP.\n\
\n\
USAGE:\n\
  context-snipe serve          Run as an MCP server over stdio (default)\n\
  context-snipe scan [PATH]    Scan a project for vulnerable dependencies\n\
  context-snipe deps [PATH]    List a project's resolved dependencies\n\
  context-snipe --version      Print version\n\
  context-snipe --help         Show this help\n",
        ver = env!("CARGO_PKG_VERSION")
    );
}
