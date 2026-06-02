//! Orchestration + human/agent-readable report formatting. Glues `deps` (what's
//! installed) to `osv` (what's vulnerable) and renders a compact text report
//! suitable both for a terminal and for returning to an LLM via MCP.

use std::path::Path;

use crate::deps;
use crate::osv::{self, Finding, Severity, Vuln};

/// `scan_dependencies`: list the resolved dependency tree.
pub fn list_dependencies(path: &str) -> Result<String, String> {
    let root = Path::new(path);
    if !root.exists() {
        return Err(format!("path does not exist: {path}"));
    }
    let sources = deps::collect(root)?;
    if sources.is_empty() {
        return Ok(no_sources_msg(root));
    }

    let mut out = format!("Dependencies in {}\n", root.display());
    let mut total = 0;
    for s in &sources {
        let kind = if s.locked { "resolved" } else { "direct" };
        out.push_str(&format!(
            "\n{} — {} {} package(s):\n",
            s.file,
            s.packages.len(),
            kind
        ));
        total += s.packages.len();
        for p in &s.packages {
            out.push_str(&format!("  {} {}  [{}]\n", p.name, p.version, p.ecosystem.label()));
        }
    }
    out.push_str(&format!(
        "\nTotal: {total} package entries across {} file(s).",
        sources.len()
    ));
    Ok(out)
}

/// `check_vulnerabilities`: cross-reference the dependency tree against OSV.dev.
pub fn check(path: &str, severity_min: Option<&str>) -> Result<String, String> {
    let root = Path::new(path);
    if !root.exists() {
        return Err(format!("path does not exist: {path}"));
    }
    let sources = deps::collect(root)?;
    if sources.is_empty() {
        return Ok(no_sources_msg(root));
    }

    let files: Vec<String> = sources.iter().map(|s| s.file.clone()).collect();
    let total_entries: usize = sources.iter().map(|s| s.packages.len()).sum();

    // Flatten + dedup across files (a monorepo can list a package twice).
    let mut all: Vec<_> = sources.into_iter().flat_map(|s| s.packages).collect();
    all.sort_by(|a, b| {
        (a.ecosystem.osv_name(), &a.name, &a.version)
            .cmp(&(b.ecosystem.osv_name(), &b.name, &b.version))
    });
    all.dedup_by(|a, b| a.ecosystem == b.ecosystem && a.name == b.name && a.version == b.version);
    let unique = all.len();

    let min = severity_min.and_then(parse_min);
    let findings = osv::check(&all)?;

    let mut report = format!(
        "context-snipe — vulnerability scan\n\
         Project: {}\n\
         Scanned: {total_entries} entries ({unique} unique packages) from {}\n",
        root.display(),
        files.join(", ")
    );

    // Apply the optional severity floor (Unknown is always kept — never hide a
    // finding just because we couldn't grade it).
    let mut shown: Vec<(&Finding, Vec<&Vuln>)> = Vec::new();
    let mut total_vulns = 0;
    for f in &findings {
        let mut vulns: Vec<&Vuln> = f
            .vulns
            .iter()
            .filter(|v| match min {
                Some(m) => v.severity == Severity::Unknown || v.severity >= m,
                None => true,
            })
            .collect();
        vulns.sort_by(|a, b| b.severity.cmp(&a.severity).then_with(|| a.id.cmp(&b.id)));
        if !vulns.is_empty() {
            total_vulns += vulns.len();
            shown.push((f, vulns));
        }
    }
    // Most severe packages first.
    shown.sort_by_key(|entry| std::cmp::Reverse(max_sev(&entry.1)));

    if shown.is_empty() {
        let floor = severity_min
            .map(|s| format!(" at/above {s} severity"))
            .unwrap_or_default();
        report.push_str(&format!("\nOK  No known vulnerabilities{floor}.\n"));
        report.push_str("\nSource: OSV.dev — advisories for packages present in your dependency tree.");
        return Ok(report);
    }

    report.push_str(&format!(
        "\nFOUND  {total_vulns} advisor{} affecting {} of {unique} package(s):\n",
        if total_vulns == 1 { "y" } else { "ies" },
        shown.len()
    ));
    for (f, vulns) in &shown {
        report.push_str(&format!(
            "\n  {} {}  [{}]\n",
            f.package.name,
            f.package.version,
            f.package.ecosystem.label()
        ));
        for v in vulns {
            let cve = v
                .aliases
                .iter()
                .find(|a| a.starts_with("CVE-"))
                .map(|c| format!(" ({c})"))
                .unwrap_or_default();
            let summary = if v.summary.is_empty() {
                String::new()
            } else {
                format!("  {}", truncate(&v.summary, 100))
            };
            report.push_str(&format!("    [{}] {}{}{}\n", v.severity.label(), v.id, cve, summary));
        }
    }
    report.push_str(
        "\nSource: OSV.dev. These advisories affect packages actually present in your \
         dependency tree.\nNote: presence is not the same as exploitability — confirm the \
         vulnerable code path is reachable in how you use the package.",
    );
    Ok(report)
}

fn no_sources_msg(root: &Path) -> String {
    format!(
        "No supported dependency files found in {}.\n\
         Supported: Cargo.lock, package-lock.json, package.json, requirements.txt, go.mod/go.sum.",
        root.display()
    )
}

fn parse_min(s: &str) -> Option<Severity> {
    match s.to_ascii_lowercase().as_str() {
        "low" => Some(Severity::Low),
        "medium" | "moderate" => Some(Severity::Medium),
        "high" => Some(Severity::High),
        "critical" => Some(Severity::Critical),
        _ => None,
    }
}

fn max_sev(vulns: &[&Vuln]) -> Severity {
    vulns.iter().map(|v| v.severity).max().unwrap_or(Severity::Unknown)
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let head: String = s.chars().take(n).collect();
        format!("{head}…")
    }
}
