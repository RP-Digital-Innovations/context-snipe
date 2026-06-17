# Context Snipe — Master Reference

Read this before touching anything. AI tools have built pieces of this across
multiple repos without leaving a map. This is the map.

---

## What Context Snipe actually is

A Windows desktop app (and supporting tools) that gives AI coding tools real
information about the developer's screen and project dependencies — so the AI
stops giving useless generic advice.

Two main features:
1. **Screen capture** — press Ctrl+Shift+X and the app grabs a screenshot of
   whatever window you're in, runs OCR on it, and makes that available to your
   AI coding tool (Cursor, Windsurf, VS Code, Zed) via MCP.
2. **Dependency/CVE scanning** — reads your lockfiles and tells your AI which
   known security vulnerabilities actually affect your project.

Business model: Free (screen capture only) / Pro $9/mo / Security $29/mo.

---

## The 4 repos and what each one does

### 1. `context-snipe-v2` (PRIVATE) — the actual product
**This is the main thing. Everything else supports it.**

A Tauri 2 desktop app that runs as a background tray icon.

- On first boot: automatically writes itself into the MCP config files of
  Cursor, Windsurf, VS Code, and Zed so the AI tools can talk to it.
- When the user presses Ctrl+Shift+X: captures the active window via DXGI
  (Windows GPU direct), runs OCR via Tesseract to extract text from the image,
  saves the result to a temp file.
- When an AI tool asks for context: the MCP server reads that temp file and
  returns the screenshot + OCR text to the AI.
- Also includes dependency scanning (reads lockfiles) and CVE checking.

Key files in `src-tauri/src/`:
- `lib.rs` — app startup, tray icon, global shortcut, daemon mode
- `capture.rs` — screen capture + OCR logic
- `ide_registrar.rs` — writes the MCP config into IDE folders on first boot
- `mcp_server.rs` — the MCP JSON-RPC server (runs when invoked with `--mcp`)
- `mcp_firewall.rs` — routes the capture result to the MCP layer

State as of June 2026: The code compiles (old error.txt files in the repo are
from earlier debugging, the current code has those fixes). Releases v0.1.0
through v0.1.2 exist in `context-snipe-releases`.

**Known gap**: License key validation is not wired up inside the app yet.
The site can issue keys (via Stripe + Supabase) but the app doesn't check them.

---

### 2. `context-snipe` (PUBLIC) — the open-source CLI companion
The standalone command-line + MCP tool for dependency/CVE scanning only.
No screen capture. No GUI. Just: read lockfiles → query OSV.dev → report CVEs.

This is what AI tools can use to answer "which packages in this project have
known vulnerabilities?"

Current version: v0.3.0 (June 2026).
- Supports Rust, npm (pnpm/yarn/package-lock), Python (poetry/uv/requirements),
  Go (go.sum/go.mod)
- MCP tools: `scan_dependencies` and `check_vulnerabilities`
- GitHub Actions CI + cross-platform release workflow set up
- Builds for: Windows x64, macOS Intel, macOS ARM, Linux x64, Linux ARM64

Relationship to V2: The open-source CLI is the free-tier version of the
security scanning feature. V2 will eventually bundle this or call it.

---

### 3. The website — TWO repos existed (a mess; being consolidated onto Astro)
Live at: https://context-snipe.rpdi.us (Cloudflare Pages project named
"context-snipe-v2").

⚠️ An AI built the marketing site TWICE, in two frameworks. Don't be fooled
(this earlier doc claimed the Astro repo was live — it was NOT):
- `context-snipe-landing` (PRIVATE, **React + Vite**) — what is CURRENTLY
  DEPLOYED to the domain. The Cloudflare project deploys from THIS repo. Being
  RETIRED.
- `context-snipe-astro` (PRIVATE, **Astro**) — the version we are standardizing
  on. As of 2026-06-17 its homepage is the open-source CLI landing page (the V2
  paid pitch was removed for now). Cloudflare must be re-pointed to deploy this
  repo; then `context-snipe-landing` is archived.

Pages (Astro repo): `index` (CLI landing), `/activate`, `/getting-started`,
`/p/[id]`, plus API routes `/api/license/{activate,validate}`, `/api/telemetry`,
`/api/waitlist`, `/api/webhook` (Stripe).

Backend stack: Supabase (license storage), Stripe (payments). Required env vars
(set on the Cloudflare Pages project): `SUPABASE_URL`, `SUPABASE_SERVICE_KEY`,
`STRIPE_WEBHOOK_SECRET`, `STRIPE_{PRO,SECURITY}_{MONTHLY,ANNUAL}_PRICE_ID`.
See `context-snipe-astro/.env.example`.

---

### 4. `context-snipe-releases` (PUBLIC) — binary distribution
Where the actual installer/exe files for V2 are published.
Current releases: v0.1.0, v0.1.1, v0.1.2.
Users download from here. Linked from the website.

---

## What is actually working right now

| Thing | Status |
|-------|--------|
| Open-source CLI (context-snipe) | Working. v0.3.0 released. CI/CD live. |
| Website | Live = `context-snipe-landing` (React). Consolidating onto `context-snipe-astro` (Astro); re-point Cloudflare, then archive React repo. |
| V2 app — screen capture | Code exists, compiled, released up to v0.1.2 |
| V2 app — IDE auto-registration | Code exists |
| V2 app — MCP server | Code exists |
| Stripe payment flow | Code exists, needs env vars confirmed |
| License gating inside the app | NOT done — app doesn't validate keys yet |
| CVE scanning inside V2 | NOT done — V2 calls the open-source CLI or will |

---

## What to work on next (in order of value)

1. **Wire license validation into V2** — the app needs to check the license key
   before enabling Pro/Security features. Without this, everyone gets everything
   for free regardless of what they paid.

2. **Confirm Stripe + Supabase are connected** — test the full purchase flow:
   click "Start Pro" → Stripe checkout → webhook fires → license created in
   Supabase → user lands on /activate → gets their key.

3. **Bundle CVE scanning into V2** — the website promises "Live CVE scanning"
   as a Security tier feature. Right now V2 doesn't call the CVE scanner.
   The open-source CLI binary could be bundled as a Tauri sidecar.

4. **Submit context-snipe (open-source) to awesome-mcp-servers lists** — free
   distribution and community signal.

---

## Architecture in plain English

```
User machine
├── context-snipe.exe (V2 Tauri app, running in background)
│   ├── Tray icon (right-click → Quit)
│   ├── Global hotkey Ctrl+Shift+X
│   │   └── Captures active window → OCR → saves to temp file
│   └── MCP server mode (--mcp flag, invoked by IDE)
│       └── Tool: fetch_latest_visual_context → returns screenshot + OCR text
│
├── Cursor / Windsurf / VS Code / Zed
│   └── MCP config (written by context-snipe on first boot)
│       └── "visual-context-mcp": { command: context-snipe.exe, args: [--mcp] }
│
└── Developer's project folder
    └── context-snipe scan . (CLI tool, separate binary)
        └── Reads lockfiles → queries api.osv.dev → returns CVE report

Cloud
├── context-snipe.rpdi.us (Astro + Cloudflare Pages)
│   ├── Marketing + pricing
│   └── /api/* (license activation, Stripe webhook)
├── Stripe (payment processing)
└── Supabase (license key storage)
```

---

## Repo quick reference

| Repo | Private? | Language | What to run |
|------|----------|----------|-------------|
| context-snipe-v2 | Yes | Rust + TypeScript (Tauri) | `cargo tauri dev` |
| context-snipe | No | Rust | `cargo build --release` |
| context-snipe-astro | Yes | Astro/TypeScript | `npm run dev` |
| context-snipe-releases | No | (binaries only) | n/a |

---

## Do not do these things

- Do not rebuild the landing page — it already exists in context-snipe-astro
- Do not create another repo — there are already too many
- Do not change the open-source CLI's "no telemetry, local-only" story — it
  is the main trust signal for the free tier
- Do not add features to the open-source CLI that belong in V2's paid tier
