<div align="center">

<h1>context-snipe</h1>

<p><strong>Your AI coding assistant doesn't know your dependencies.<br/>It's guessing. This fixes that.</strong></p>

[![CI](https://github.com/RP-Digital-Innovations/context-snipe/actions/workflows/ci.yml/badge.svg)](https://github.com/RP-Digital-Innovations/context-snipe/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![GitHub release](https://img.shields.io/github/v/release/RP-Digital-Innovations/context-snipe)](https://github.com/RP-Digital-Innovations/context-snipe/releases/latest)
[![MCP](https://img.shields.io/badge/MCP-compatible-blueviolet)](https://modelcontextprotocol.io)

<p>A ~1 MB pure-Rust binary that reads your lockfiles, cross-references every package against OSV.dev, and hands your AI a short, ranked, accurate vulnerability report — over the Model Context Protocol.</p>

**Works with:** Claude Desktop · Cursor · Windsurf · VS Code · Zed · any MCP client

---

</div>

```
$ context-snipe scan .

context-snipe — vulnerability scan
Project: ./my-api
Scanned: 412 entries (388 unique packages) from Cargo.lock, package-lock.json

FOUND  3 advisories affecting 2 of 388 package(s):

  lodash 4.17.11  [npm]
    [CRIT] CVE-2019-10744  Prototype Pollution in lodash
    [HIGH] CVE-2021-23337  Command Injection in lodash

  minimatch 3.0.4  [npm]
    [HIGH] CVE-2022-3517   minimatch ReDoS vulnerability

Source: OSV.dev — packages actually in your resolved dependency tree.
```

---

## The problem

You ask Cursor or Claude: *"Does my project have any security issues?"*

It doesn't know your packages. It doesn't know your versions. It hallucinates an answer based on general knowledge — not your actual `package-lock.json`.

Your scanner (Dependabot, Snyk, whatever) floods you with 200 warnings, most of which don't apply to what you actually ship. You spend 45 minutes Googling CVEs that are irrelevant to your code.

**context-snipe closes both gaps.** It reads your *resolved* lockfiles (not your `package.json` — your actual installed packages), asks OSV.dev only about what you have, deduplicates the noise, ranks by real CVSS severity, and gives your AI a clean, accurate briefing it can actually reason about.

---

## Install in 30 seconds

**macOS / Linux** — one line, picks the right binary for your platform:
```bash
curl -fsSL https://raw.githubusercontent.com/RP-Digital-Innovations/context-snipe/main/install.sh | sh
```

**Windows** (PowerShell):
```powershell
irm https://raw.githubusercontent.com/RP-Digital-Innovations/context-snipe/main/install.ps1 | iex
```

**Rust users** — from [crates.io](https://crates.io/crates/context-snipe):
```bash
cargo install context-snipe        # build from source
cargo binstall context-snipe       # or grab the prebuilt binary, no compile
```

<details>
<summary>Manual download</summary>

Grab the binary for your platform from the [latest release](https://github.com/RP-Digital-Innovations/context-snipe/releases/latest):

| Platform | Asset |
|----------|-------|
| macOS (Apple Silicon) | `context-snipe-aarch64-apple-darwin` |
| macOS (Intel) | `context-snipe-x86_64-apple-darwin` |
| Linux x86_64 | `context-snipe-x86_64-linux` |
| Linux ARM64 | `context-snipe-aarch64-linux` |
| Windows x86_64 | `context-snipe-x86_64-pc-windows.exe` |

`chmod +x` it and move it onto your PATH.
</details>

**Verify:**
```bash
context-snipe --version
```

---

## Add to your AI tool (60 seconds)

### Claude Desktop

Add to `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS) or `%APPDATA%\Claude\claude_desktop_config.json` (Windows):

```json
{
  "mcpServers": {
    "context-snipe": {
      "command": "context-snipe",
      "args": ["serve"]
    }
  }
}
```

### Cursor

Add to `~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "context-snipe": {
      "command": "context-snipe",
      "args": ["serve"]
    }
  }
}
```

### Windsurf

Add to `~/.codeium/windsurf/mcp_config.json`:

```json
{
  "mcpServers": {
    "context-snipe": {
      "command": "context-snipe",
      "args": ["serve"]
    }
  }
}
```

**Restart your editor.** Then ask your AI: *"Check this project for vulnerable dependencies."*

---

## What your AI can now do

| MCP Tool | What it does |
|----------|-------------|
| `scan_dependencies` | Lists every resolved package in your project (name, version, ecosystem) |
| `check_vulnerabilities` | Cross-references your packages against OSV.dev — returns only advisories that affect what you actually have installed |

Your AI goes from guessing to knowing. In one tool call.

---

## Supported ecosystems

| Ecosystem | Resolved lockfile (preferred) | Fallback |
|-----------|-------------------------------|---------|
| **Rust** | `Cargo.lock` | — |
| **npm** | `pnpm-lock.yaml`, `yarn.lock`, `package-lock.json` v1–v3 | `package.json` |
| **Python** | `poetry.lock`, `uv.lock` | `requirements.txt` (pinned `==`) |
| **Go** | `go.sum` | `go.mod` |

---

## How it compares

| | context-snipe | Dependabot | Snyk | socket.dev |
|--|:--:|:--:|:--:|:--:|
| MCP native — AI gets the results directly | ✅ | ❌ | ❌ | ❌ |
| Reads resolved lockfiles (not just manifests) | ✅ | ✅ | ✅ | ✅ |
| 100% local — nothing leaves your machine | ✅ | ❌ | ❌ | ❌ |
| No account, no signup, no API key | ✅ | ❌ | ❌ | ❌ |
| Binary size | ~1 MB | N/A | 200 MB+ | N/A |
| Free, open source | ✅ | ✅ | Partial | Partial |

---

## CLI usage

```bash
context-snipe scan [PATH]    # vulnerability report (defaults to current dir)
context-snipe deps [PATH]    # list the full resolved dependency tree
context-snipe serve          # start the MCP server over stdio
context-snipe --help
```

---

## A note on honesty

context-snipe tells you which vulnerable packages are **present in your resolved dependency tree**. It does not perform call-graph reachability analysis. Presence is not proof of exploitability — the vulnerable function may not be reachable in your code. The tool says so in its own output, by design.

No tool that runs in seconds can tell you a CVE is definitely not exploitable. We won't pretend otherwise.

---

## How it works

- **MCP engine** — hand-rolled JSON-RPC 2.0 over stdio. `initialize`, `tools/list`, `tools/call`, `ping`. stdout is the protocol channel; all diagnostics go to stderr.
- **Lockfile parsers** — TOML for Cargo, JSON for npm, custom parsers for pnpm/yarn, line parsers for requirements.txt and Go modules.
- **OSV client** — one `querybatch` call filters the full tree to packages with advisories, then a focused `query` per hit pulls details. CVSS v3.x base scores computed from vector strings. Duplicate advisories sharing a CVE are merged.
- **TLS via rustls** — pure-Rust, no OpenSSL, no system crypto dependency. Works identically on Windows, macOS, and musl Linux.

---

## Build from source

```bash
cargo build --release
# Binary at: target/release/context-snipe
```

Requires stable Rust. The release profile statically links the CRT — the binary is fully self-contained.

---

## Roadmap

- [ ] GitHub App — post CVE diffs on pull requests (shows what a PR *introduces*)
- [ ] Policy layer — configurable CI failure thresholds per severity
- [ ] More ecosystems — Ruby (Gemfile.lock), PHP (composer.lock), Java (pom.xml)

---

## Contributing

PRs welcome. The codebase is ~1,000 lines of Rust split across:

```
src/
  main.rs    — CLI entry, mode routing
  mcp.rs     — JSON-RPC / MCP server
  deps.rs    — lockfile parsers
  osv.rs     — OSV.dev client + CVSS scoring
  scan.rs    — orchestration + report formatting
  http.rs    — ureq + rustls HTTP agent
```

Good first issues: adding a new lockfile format, improving CVSS display, adding output formats (JSON, SARIF).

---

## License

MIT — free forever. No telemetry. No accounts. No cloud.

<div align="center">
<br/>
<a href="https://context-snipe.rpdi.us">Website</a> · <a href="https://github.com/RP-Digital-Innovations/context-snipe/releases">Releases</a> · <a href="https://github.com/RP-Digital-Innovations/context-snipe/issues">Issues</a>
<br/><br/>
Built by <a href="https://rpdi.us">RP Digital Innovations</a>
</div>
