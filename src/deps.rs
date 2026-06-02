//! Dependency discovery: detect manifests/lockfiles in a project directory and
//! parse them into a flat list of (name, version, ecosystem) packages.
//!
//! Lockfiles (Cargo.lock, package-lock.json, go.sum) give the fully *resolved*
//! tree — exact transitive versions — which is what OSV needs for precise hits.
//! Plain manifests (package.json, requirements.txt, go.mod) are best-effort
//! direct-dependency fallbacks when no lockfile is present.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub ecosystem: Ecosystem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ecosystem {
    CratesIo,
    Npm,
    PyPI,
    Go,
}

impl Ecosystem {
    /// The exact ecosystem string OSV.dev expects.
    pub fn osv_name(&self) -> &'static str {
        match self {
            Ecosystem::CratesIo => "crates.io",
            Ecosystem::Npm => "npm",
            Ecosystem::PyPI => "PyPI",
            Ecosystem::Go => "Go",
        }
    }

    /// Short label for human-facing output.
    pub fn label(&self) -> &'static str {
        match self {
            Ecosystem::CratesIo => "cargo",
            Ecosystem::Npm => "npm",
            Ecosystem::PyPI => "pip",
            Ecosystem::Go => "go",
        }
    }
}

/// A detected dependency file and the packages parsed from it.
pub struct Source {
    pub file: String,
    /// True if parsed from a lockfile (exact, fully-resolved tree).
    pub locked: bool,
    pub packages: Vec<Package>,
}

/// Scan `root` for supported dependency files and parse each one found.
pub fn collect(root: &Path) -> Result<Vec<Source>, String> {
    let mut sources = Vec::new();

    // --- Rust ---
    let cargo_lock = root.join("Cargo.lock");
    if cargo_lock.is_file() {
        sources.push(Source {
            file: "Cargo.lock".into(),
            locked: true,
            packages: parse_cargo_lock(&cargo_lock)?,
        });
    }

    // --- npm (prefer the lockfile) ---
    let pkg_lock = root.join("package-lock.json");
    let pkg_json = root.join("package.json");
    if pkg_lock.is_file() {
        sources.push(Source {
            file: "package-lock.json".into(),
            locked: true,
            packages: parse_package_lock(&pkg_lock)?,
        });
    } else if pkg_json.is_file() {
        sources.push(Source {
            file: "package.json".into(),
            locked: false,
            packages: parse_package_json(&pkg_json)?,
        });
    }

    // --- Python ---
    let req = root.join("requirements.txt");
    if req.is_file() {
        sources.push(Source {
            file: "requirements.txt".into(),
            locked: false,
            packages: parse_requirements(&req)?,
        });
    }

    // --- Go (prefer go.sum) ---
    let go_sum = root.join("go.sum");
    let go_mod = root.join("go.mod");
    if go_sum.is_file() {
        sources.push(Source {
            file: "go.sum".into(),
            locked: true,
            packages: parse_go_sum(&go_sum)?,
        });
    } else if go_mod.is_file() {
        sources.push(Source {
            file: "go.mod".into(),
            locked: false,
            packages: parse_go_mod(&go_mod)?,
        });
    }

    Ok(sources)
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn parse_cargo_lock(path: &Path) -> Result<Vec<Package>, String> {
    let value: toml::Value =
        toml::from_str(&read(path)?).map_err(|e| format!("Cargo.lock: {e}"))?;
    let mut out = Vec::new();
    if let Some(pkgs) = value.get("package").and_then(|v| v.as_array()) {
        for p in pkgs {
            if let (Some(name), Some(ver)) = (
                p.get("name").and_then(|v| v.as_str()),
                p.get("version").and_then(|v| v.as_str()),
            ) {
                out.push(Package {
                    name: name.to_string(),
                    version: ver.to_string(),
                    ecosystem: Ecosystem::CratesIo,
                });
            }
        }
    }
    Ok(out)
}

fn parse_package_lock(path: &Path) -> Result<Vec<Package>, String> {
    let v: serde_json::Value =
        serde_json::from_str(&read(path)?).map_err(|e| format!("package-lock.json: {e}"))?;
    let mut out = Vec::new();

    // lockfileVersion 2/3: flat `packages` map keyed by install path.
    if let Some(pkgs) = v.get("packages").and_then(|v| v.as_object()) {
        for (key, val) in pkgs {
            if key.is_empty() {
                continue; // the root project itself
            }
            // name is the segment after the final "node_modules/"
            let name = key.rsplit("node_modules/").next().unwrap_or(key);
            if name.is_empty() {
                continue;
            }
            if let Some(ver) = val.get("version").and_then(|x| x.as_str()) {
                out.push(Package {
                    name: name.to_string(),
                    version: ver.to_string(),
                    ecosystem: Ecosystem::Npm,
                });
            }
        }
    }

    // lockfileVersion 1: nested `dependencies` tree.
    if out.is_empty() {
        if let Some(deps) = v.get("dependencies").and_then(|v| v.as_object()) {
            collect_npm_v1(deps, &mut out);
        }
    }
    Ok(out)
}

fn collect_npm_v1(deps: &serde_json::Map<String, serde_json::Value>, out: &mut Vec<Package>) {
    for (name, val) in deps {
        if let Some(ver) = val.get("version").and_then(|x| x.as_str()) {
            out.push(Package {
                name: name.clone(),
                version: ver.to_string(),
                ecosystem: Ecosystem::Npm,
            });
        }
        if let Some(nested) = val.get("dependencies").and_then(|x| x.as_object()) {
            collect_npm_v1(nested, out);
        }
    }
}

fn parse_package_json(path: &Path) -> Result<Vec<Package>, String> {
    let v: serde_json::Value =
        serde_json::from_str(&read(path)?).map_err(|e| format!("package.json: {e}"))?;
    let mut out = Vec::new();
    for field in ["dependencies", "devDependencies", "optionalDependencies"] {
        if let Some(deps) = v.get(field).and_then(|x| x.as_object()) {
            for (name, ver) in deps {
                if let Some(clean) = ver.as_str().and_then(clean_semver) {
                    out.push(Package {
                        name: name.clone(),
                        version: clean,
                        ecosystem: Ecosystem::Npm,
                    });
                }
            }
        }
    }
    Ok(out)
}

/// Strip a leading range operator (^, ~, >=, etc.) and keep the first concrete
/// `x.y.z`. Returns None for ranges we can't pin (`*`, `latest`, git/url specs).
fn clean_semver(s: &str) -> Option<String> {
    let s = s.trim();
    let start = s.find(|c: char| c.is_ascii_digit())?;
    let rest = &s[start..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    let v = &rest[..end];
    if v.is_empty() || !v.contains('.') {
        None
    } else {
        Some(v.to_string())
    }
}

fn parse_requirements(path: &Path) -> Result<Vec<Package>, String> {
    let text = read(path)?;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('-') {
            continue;
        }
        // Only pinned exact versions (`name==1.2.3`) can be queried reliably.
        if let Some(idx) = line.find("==") {
            let name_part = &line[..idx];
            let name = name_part.split('[').next().unwrap_or(name_part).trim();
            let after = &line[idx + 2..];
            let ver: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-' || *c == '+')
                .collect();
            if !name.is_empty() && !ver.is_empty() {
                out.push(Package {
                    name: name.to_string(),
                    version: ver,
                    ecosystem: Ecosystem::PyPI,
                });
            }
        }
    }
    Ok(out)
}

fn parse_go_sum(path: &Path) -> Result<Vec<Package>, String> {
    let text = read(path)?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(module), Some(ver)) = (parts.next(), parts.next()) else {
            continue;
        };
        if ver.ends_with("/go.mod") {
            continue; // the go.mod hash line, not the module itself
        }
        let ver = ver.trim_start_matches('v');
        if seen.insert(format!("{module}@{ver}")) {
            out.push(Package {
                name: module.to_string(),
                version: ver.to_string(),
                ecosystem: Ecosystem::Go,
            });
        }
    }
    Ok(out)
}

fn parse_go_mod(path: &Path) -> Result<Vec<Package>, String> {
    let text = read(path)?;
    let mut out = Vec::new();
    let mut in_block = false;
    for raw in text.lines() {
        let l = raw.trim();
        if l.starts_with("require (") {
            in_block = true;
            continue;
        }
        if in_block && l == ")" {
            in_block = false;
            continue;
        }
        let l = if let Some(rest) = l.strip_prefix("require ") {
            rest.trim()
        } else if in_block {
            l
        } else {
            continue;
        };
        let l = l.split("//").next().unwrap_or(l).trim(); // drop `// indirect`
        let mut parts = l.split_whitespace();
        if let (Some(module), Some(ver)) = (parts.next(), parts.next()) {
            let ver = ver.trim_start_matches('v');
            if !module.is_empty() && ver.starts_with(|c: char| c.is_ascii_digit()) {
                out.push(Package {
                    name: module.to_string(),
                    version: ver.to_string(),
                    ecosystem: Ecosystem::Go,
                });
            }
        }
    }
    Ok(out)
}
