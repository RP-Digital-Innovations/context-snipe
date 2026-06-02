# context-snipe

**Deterministic dependency + CVE context for AI coding tools, over the [Model Context Protocol](https://modelcontextprotocol.io).**

A single, ~0.85 MB, pure-Rust binary that gives an AI assistant ground truth about a project's dependencies — and tells it *which CVEs actually affect packages in the dependency tree*, instead of generic scanner noise. No Electron, no runtime, no telemetry. It runs locally and speaks MCP over stdio, so it drops straight into Claude Desktop, Cursor, or any MCP client.

```
$ context-snipe scan ./my-api
context-snipe — vulnerability scan
Project: ./my-api
Scanned: 412 entries (388 unique packages) from Cargo.lock, package-lock.json

FOUND  3 advisories affecting 2 of 388 package(s):

  lodash 4.17.11  [npm]
    [CRIT] GHSA-jf85-cpcp-j695 (CVE-2019-10744)  Prototype Pollution in lodash
    [HIGH] GHSA-35jh-r3h4-6jhm (CVE-2021-23337)  Command Injection in lodash

  minimatch 3.0.4  [npm]
    [HIGH] GHSA-f8q6-p94x-37v3 (CVE-2022-3517)  minimatch ReDoS vulnerability

Source: OSV.dev. These advisories affect packages actually present in your dependency tree.
Note: presence is not the same as exploitability — confirm the vulnerable code path is reachable in how you use the package.
```

## Why

A typical dependency scanner dumps hundreds of advisories, most of which don't apply to the code you actually ship. When you then ask an AI assistant about them, it has no idea what's in your lockfile, so it guesses. context-snipe closes that gap: it parses your *resolved* dependency tree, asks [OSV.dev](https://osv.dev) only about packages you actually depend on, collapses the GHSA/PYSEC/CVE duplicates OSV returns into one finding per real vulnerability, grades each by a CVSS base score computed from its vector, and hands the assistant a short, ranked, deterministic report.

## MCP tools

| Tool | What it does |
|------|--------------|
| `scan_dependencies` | Lists the resolved dependency tree (name, version, ecosystem) from the project's lockfiles/manifests. |
| `check_vulnerabilities` | Cross-references that tree against OSV.dev and reports the advisories that affect it, with computed severity. Optional `severity_min` floor. |

## Supported ecosystems

| Ecosystem | Resolved (preferred) | Direct-only fallback |
|-----------|----------------------|----------------------|
| Rust | `Cargo.lock` | — |
| npm | `package-lock.json` (v1/v2/v3) | `package.json` |
| Python | — | `requirements.txt` (pinned `==`) |
| Go | `go.sum` | `go.mod` |

## Usage

### As an MCP server

Point any MCP client at the binary in `serve` mode. Claude Desktop (`claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "context-snipe": {
      "command": "C:\\path\\to\\context-snipe.exe",
      "args": ["serve"]
    }
  }
}
```

Cursor (`~/.cursor/mcp.json`) uses the same `command` / `args` shape. Restart the client, then ask: *"check this project for vulnerable dependencies."*

### As a CLI

```
context-snipe scan [PATH]    # vulnerability report (defaults to .)
context-snipe deps [PATH]    # list the resolved dependency tree
context-snipe --help
```

## How it works

- **Hand-rolled JSON-RPC 2.0 / MCP engine** (`src/mcp.rs`) — newline-delimited messages over stdio, implementing `initialize`, `tools/list`, `tools/call`, and `ping`. stdout is the protocol channel; all diagnostics go to stderr. Tolerates a leading UTF-8 BOM.
- **Lockfile parsers** (`src/deps.rs`) — TOML for Cargo, JSON for npm (both lockfile layouts), line parsers for `requirements.txt` and Go modules.
- **OSV client** (`src/osv.rs`) — one `querybatch` call filters the full tree down to packages with advisories, then a focused `query` per hit pulls details. Severity is the database's own grade where available, else a CVSS v3.x base score computed from the vector string. Duplicate advisories sharing a CVE are merged, keeping the richest record.
- **TLS via the OS** — `ureq` over native-tls (SChannel on Windows), so the binary needs no OpenSSL and no bundled crypto.

## Build

Requires a Rust toolchain. On Windows with the GNU toolchain you also need MinGW binutils on `PATH` (for `dlltool`/`as`); the MSVC toolchain needs no extra setup.

```
cargo build --release
```

The release profile statically links the CRT (`.cargo/config.toml`) so the resulting `.exe` is self-contained — it launches when spawned by an MCP client that has no toolchain on its `PATH`.

## Scope & honesty

This reports vulnerabilities for packages **present in your resolved dependency tree**. It does **not** perform call-graph reachability analysis — presence is not proof of exploitability. The tool says so in its own output, by design.

## Roadmap

- Screen-capture + OCR as a second MCP tool (native Windows capture)
- More lockfiles: `pnpm-lock.yaml`, `yarn.lock`, `poetry.lock`, `uv.lock`
- Tauri tray app: autostart, one-click IDE registration, packaged `.msi`
- macOS / Linux targets

## License

MIT
