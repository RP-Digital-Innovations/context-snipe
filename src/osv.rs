//! OSV.dev client.
//!
//! Two-phase to stay cheap: a single `querybatch` call filters the full
//! dependency set down to packages that have *any* advisory, then a focused
//! `query` per hit pulls full advisory details (summary, severity, aliases).

use serde_json::{json, Value};

use crate::deps::Package;

const OSV_BATCH: &str = "https://api.osv.dev/v1/querybatch";
const OSV_QUERY: &str = "https://api.osv.dev/v1/query";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Unknown,
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub fn label(&self) -> &'static str {
        match self {
            Severity::Critical => "CRIT",
            Severity::High => "HIGH",
            Severity::Medium => "MED ",
            Severity::Low => "LOW ",
            Severity::Unknown => "??? ",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Vuln {
    pub id: String,
    pub summary: String,
    pub severity: Severity,
    pub aliases: Vec<String>,
}

pub struct Finding {
    pub package: Package,
    pub vulns: Vec<Vuln>,
}

/// Query OSV for `packages`; returns only the packages that have >=1 advisory.
pub fn check(packages: &[Package]) -> Result<Vec<Finding>, String> {
    if packages.is_empty() {
        return Ok(Vec::new());
    }

    let queries: Vec<Value> = packages
        .iter()
        .map(|p| {
            json!({
                "package": { "name": p.name, "ecosystem": p.ecosystem.osv_name() },
                "version": p.version
            })
        })
        .collect();

    let mut findings = Vec::new();
    // OSV caps querybatch at 1000 queries; stay well under it.
    for (pkg_chunk, q_chunk) in packages.chunks(500).zip(queries.chunks(500)) {
        let resp = crate::http::post_json(OSV_BATCH, &json!({ "queries": q_chunk }))?;
        let results = resp
            .get("results")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        for (i, res) in results.iter().enumerate() {
            let has_vulns = res
                .get("vulns")
                .and_then(|v| v.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false);
            if !has_vulns {
                continue;
            }
            let Some(pkg) = pkg_chunk.get(i) else { continue };
            let vulns = query_one(pkg)?;
            if !vulns.is_empty() {
                findings.push(Finding {
                    package: pkg.clone(),
                    vulns,
                });
            }
        }
    }
    Ok(findings)
}

fn query_one(pkg: &Package) -> Result<Vec<Vuln>, String> {
    let body = json!({
        "package": { "name": pkg.name, "ecosystem": pkg.ecosystem.osv_name() },
        "version": pkg.version
    });
    let resp = crate::http::post_json(OSV_QUERY, &body)?;
    let vulns = resp
        .get("vulns")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(dedup_by_cve(vulns.iter().map(parse_vuln).collect()))
}

/// OSV emits one record per database (GHSA + PYSEC + CVE) for the *same*
/// underlying flaw. Collapse them by shared CVE so we report each real
/// vulnerability once, keeping the richest twin (best id + summary + severity).
fn dedup_by_cve(vulns: Vec<Vuln>) -> Vec<Vuln> {
    use std::collections::HashMap;
    let mut order: Vec<String> = Vec::new();
    let mut map: HashMap<String, Vuln> = HashMap::new();
    for v in vulns {
        let key = v
            .aliases
            .iter()
            .find(|a| a.starts_with("CVE-"))
            .cloned()
            .unwrap_or_else(|| v.id.clone());
        match map.remove(&key) {
            Some(existing) => {
                map.insert(key, merge(existing, v));
            }
            None => {
                order.push(key.clone());
                map.insert(key, v);
            }
        }
    }
    order.into_iter().filter_map(|k| map.remove(&k)).collect()
}

fn merge(a: Vuln, b: Vuln) -> Vuln {
    // Lower rank wins as the representative; merge in the other's data.
    let (mut keep, drop) = if rank(&a) <= rank(&b) { (a, b) } else { (b, a) };
    keep.severity = keep.severity.max(drop.severity);
    if keep.summary.is_empty() {
        keep.summary = drop.summary;
    }
    for alias in drop.aliases {
        if !keep.aliases.contains(&alias) {
            keep.aliases.push(alias);
        }
    }
    keep
}

/// Prefer GHSA/RUSTSEC ids (curated, carry severity) over bare CVE/PYSEC; break
/// ties toward the entry with the longer summary.
fn rank(v: &Vuln) -> (u8, std::cmp::Reverse<usize>) {
    let id_rank = if v.id.starts_with("GHSA") {
        0
    } else if v.id.starts_with("RUSTSEC") {
        1
    } else if v.id.starts_with("GO-") {
        2
    } else if v.id.starts_with("CVE") {
        3
    } else if v.id.starts_with("PYSEC") {
        5
    } else {
        4
    };
    (id_rank, std::cmp::Reverse(v.summary.len()))
}

fn parse_vuln(v: &Value) -> Vuln {
    let id = v
        .get("id")
        .and_then(|x| x.as_str())
        .unwrap_or("UNKNOWN")
        .to_string();
    let summary = v
        .get("summary")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("details").and_then(|x| x.as_str()))
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("")
        .to_string();
    let aliases = v
        .get("aliases")
        .and_then(|x| x.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    Vuln {
        id,
        summary,
        severity: parse_severity(v),
        aliases,
    }
}

/// Best-effort severity: prefer the database's own word (GHSA), fall back to a
/// CVSS base score computed from the vector string.
fn parse_severity(v: &Value) -> Severity {
    if let Some(word) = v
        .get("database_specific")
        .and_then(|d| d.get("severity"))
        .and_then(|x| x.as_str())
    {
        if let Some(sev) = severity_from_word(word) {
            return sev;
        }
    }
    if let Some(arr) = v.get("severity").and_then(|x| x.as_array()) {
        for item in arr {
            if let Some(score) = item.get("score").and_then(|x| x.as_str()) {
                if let Ok(num) = score.parse::<f32>() {
                    return severity_from_cvss(num);
                }
                if let Some(num) = cvss_base_from_vector(score) {
                    return severity_from_cvss(num);
                }
            }
        }
    }
    Severity::Unknown
}

fn severity_from_word(s: &str) -> Option<Severity> {
    match s.to_ascii_uppercase().as_str() {
        "CRITICAL" => Some(Severity::Critical),
        "HIGH" => Some(Severity::High),
        "MODERATE" | "MEDIUM" => Some(Severity::Medium),
        "LOW" => Some(Severity::Low),
        _ => None,
    }
}

fn severity_from_cvss(score: f32) -> Severity {
    if score >= 9.0 {
        Severity::Critical
    } else if score >= 7.0 {
        Severity::High
    } else if score >= 4.0 {
        Severity::Medium
    } else if score > 0.0 {
        Severity::Low
    } else {
        Severity::Unknown
    }
}

/// Compute a CVSS v3.x base score from a vector string. Returns None for
/// non-v3 vectors or anything malformed (caller treats that as Unknown).
fn cvss_base_from_vector(vec: &str) -> Option<f32> {
    if !vec.contains("CVSS:3") {
        return None;
    }
    let (mut av, mut ac, mut pr, mut ui) = (None, None, None, None);
    let (mut scope, mut c, mut i, mut a) = (None, None, None, None);
    for part in vec.split('/') {
        let mut kv = part.splitn(2, ':');
        let (Some(k), Some(val)) = (kv.next(), kv.next()) else {
            continue;
        };
        match k {
            "AV" => av = Some(val),
            "AC" => ac = Some(val),
            "PR" => pr = Some(val),
            "UI" => ui = Some(val),
            "S" => scope = Some(val),
            "C" => c = Some(val),
            "I" => i = Some(val),
            "A" => a = Some(val),
            _ => {}
        }
    }
    let changed = scope? == "C";
    let av = match av? {
        "N" => 0.85,
        "A" => 0.62,
        "L" => 0.55,
        "P" => 0.20,
        _ => return None,
    };
    let ac = match ac? {
        "L" => 0.77,
        "H" => 0.44,
        _ => return None,
    };
    let ui = match ui? {
        "N" => 0.85,
        "R" => 0.62,
        _ => return None,
    };
    let pr = match pr? {
        "N" => 0.85,
        "L" => {
            if changed {
                0.68
            } else {
                0.62
            }
        }
        "H" => {
            if changed {
                0.50
            } else {
                0.27
            }
        }
        _ => return None,
    };
    let impact_metric = |x: &str| match x {
        "H" => Some(0.56_f32),
        "L" => Some(0.22),
        "N" => Some(0.0),
        _ => None,
    };
    let (cc, ii, aa) = (impact_metric(c?)?, impact_metric(i?)?, impact_metric(a?)?);

    let isc_base = 1.0 - ((1.0 - cc) * (1.0 - ii) * (1.0 - aa));
    let impact = if changed {
        7.52 * (isc_base - 0.029) - 3.25 * (isc_base - 0.02).powf(15.0)
    } else {
        6.42 * isc_base
    };
    if impact <= 0.0 {
        return Some(0.0);
    }
    let exploitability = 8.22 * av * ac * pr * ui;
    let raw = if changed {
        (1.08 * (impact + exploitability)).min(10.0)
    } else {
        (impact + exploitability).min(10.0)
    };
    Some((raw * 10.0).ceil() / 10.0) // CVSS round-up to 1 decimal
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vuln(id: &str, sev: Severity, cve: &str, summary: &str) -> Vuln {
        Vuln {
            id: id.into(),
            summary: summary.into(),
            severity: sev,
            aliases: vec![cve.into()],
        }
    }

    #[test]
    fn cvss_log4shell_scores_10() {
        // CVE-2021-44228, published base score 10.0 (scope-changed branch).
        let s = cvss_base_from_vector("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:C/C:H/I:H/A:H").unwrap();
        assert!((s - 10.0).abs() < 0.05, "expected 10.0, got {s}");
        assert_eq!(severity_from_cvss(s), Severity::Critical);
    }

    #[test]
    fn cvss_scope_unchanged_scores_7_5() {
        // Network info-disclosure, published base score 7.5 (scope-unchanged branch).
        let s = cvss_base_from_vector("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:N/A:N").unwrap();
        assert!((s - 7.5).abs() < 0.05, "expected 7.5, got {s}");
        assert_eq!(severity_from_cvss(s), Severity::High);
    }

    #[test]
    fn cvss_rejects_non_v3_and_garbage() {
        assert!(cvss_base_from_vector("CVSS:2.0/AV:N/AC:L/Au:N/C:P/I:P/A:P").is_none());
        assert!(cvss_base_from_vector("not-a-vector").is_none());
        assert!(cvss_base_from_vector("CVSS:3.1/AV:X/AC:L/PR:N/UI:N/S:U/C:H/I:N/A:N").is_none());
    }

    #[test]
    fn severity_thresholds() {
        assert_eq!(severity_from_cvss(9.0), Severity::Critical);
        assert_eq!(severity_from_cvss(8.9), Severity::High);
        assert_eq!(severity_from_cvss(4.0), Severity::Medium);
        assert_eq!(severity_from_cvss(0.1), Severity::Low);
        assert_eq!(severity_from_cvss(0.0), Severity::Unknown);
    }

    #[test]
    fn dedup_collapses_ghsa_and_pysec_sharing_a_cve() {
        let pysec = vuln("PYSEC-2020-1", Severity::Unknown, "CVE-2020-0001", "");
        let ghsa = vuln("GHSA-aaaa", Severity::High, "CVE-2020-0001", "Real summary");
        let out = dedup_by_cve(vec![pysec, ghsa]);
        assert_eq!(out.len(), 1, "GHSA + PYSEC for one CVE must collapse to one");
        assert_eq!(out[0].id, "GHSA-aaaa", "GHSA is the preferred representative");
        assert_eq!(out[0].severity, Severity::High, "higher severity is retained");
        assert_eq!(out[0].summary, "Real summary", "non-empty summary is retained");
    }

    #[test]
    fn dedup_keeps_distinct_cves_separate() {
        let a = vuln("GHSA-a", Severity::Low, "CVE-1", "");
        let b = vuln("GHSA-b", Severity::Low, "CVE-2", "");
        assert_eq!(dedup_by_cve(vec![a, b]).len(), 2);
    }
}
