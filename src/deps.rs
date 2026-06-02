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

    // --- npm (prefer the most precise lockfile available) ---
    let pnpm = root.join("pnpm-lock.yaml");
    let yarn = root.join("yarn.lock");
    let pkg_lock = root.join("package-lock.json");
    let pkg_json = root.join("package.json");
    if pnpm.is_file() {
        sources.push(Source {
            file: "pnpm-lock.yaml".into(),
            locked: true,
            packages: parse_pnpm_lock(&pnpm)?,
        });
    } else if yarn.is_file() {
        sources.push(Source {
            file: "yarn.lock".into(),
            locked: true,
            packages: parse_yarn_lock(&yarn)?,
        });
    } else if pkg_lock.is_file() {
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

    // --- Python (prefer a lockfile over requirements.txt) ---
    let poetry = root.join("poetry.lock");
    let uv = root.join("uv.lock");
    let req = root.join("requirements.txt");
    if poetry.is_file() {
        sources.push(Source {
            file: "poetry.lock".into(),
            locked: true,
            packages: parse_pep_lock(&poetry)?,
        });
    } else if uv.is_file() {
        sources.push(Source {
            file: "uv.lock".into(),
            locked: true,
            packages: parse_pep_lock(&uv)?,
        });
    } else if req.is_file() {
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

/// Parse poetry.lock / uv.lock (TOML with `[[package]]` name + version).
fn parse_pep_lock(path: &Path) -> Result<Vec<Package>, String> {
    let value: toml::Value =
        toml::from_str(&read(path)?).map_err(|e| format!("{}: {e}", path.display()))?;
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
                    ecosystem: Ecosystem::PyPI,
                });
            }
        }
    }
    Ok(out)
}

/// Parse pnpm-lock.yaml by reading the keys of the top-level `packages:` block
/// (each key encodes `name@version`). Avoids a full YAML dependency.
fn parse_pnpm_lock(path: &Path) -> Result<Vec<Package>, String> {
    let text = read(path)?;
    let mut out = Vec::new();
    let mut in_packages = false;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent == 0 {
            in_packages = line.trim_end() == "packages:";
            continue;
        }
        // Package entries sit one level (2 spaces) under `packages:`, ending ':'.
        if in_packages && indent == 2 {
            if let Some(key) = line.trim_end().strip_suffix(':') {
                if let Some((name, version)) = parse_pnpm_key(key) {
                    out.push(Package {
                        name,
                        version,
                        ecosystem: Ecosystem::Npm,
                    });
                }
            }
        }
    }
    Ok(out)
}

/// Extract (name, version) from a pnpm package key, handling scoped names,
/// quoting, a leading slash (v5/v6), peer-dependency suffixes `(...)`, and the
/// older `name/version` separator.
fn parse_pnpm_key(raw: &str) -> Option<(String, String)> {
    let k = raw.trim().trim_matches('\'').trim_matches('"');
    let k = k.strip_prefix('/').unwrap_or(k);
    let k = match k.find('(') {
        Some(i) => &k[..i],
        None => k,
    };
    let (name, version) = match k.rfind('@').filter(|&i| i > 0) {
        Some(i) => (&k[..i], &k[i + 1..]),
        None => {
            let i = k.rfind('/')?;
            (&k[..i], &k[i + 1..])
        }
    };
    if name.is_empty() || !version.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    Some((name.to_string(), version.to_string()))
}

/// Parse yarn.lock (classic and Berry). Each entry header names one or more
/// specs; the following indented `version` line gives the resolved version.
fn parse_yarn_lock(path: &Path) -> Result<Vec<Package>, String> {
    let text = read(path)?;
    let mut out = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent == 0 {
            current = line
                .trim_end()
                .strip_suffix(':')
                .and_then(|specs| specs.split(',').next())
                .and_then(yarn_name);
        } else if let Some(name) = current.clone() {
            if let Some(version) = yarn_version(trimmed) {
                out.push(Package {
                    name,
                    version,
                    ecosystem: Ecosystem::Npm,
                });
                current = None; // consumed this entry
            }
        }
    }
    Ok(out)
}

/// Package name from a yarn spec: `lodash@^4.17.11`, `"@scope/x@^1.0.0"`, or
/// Berry's `"lodash@npm:^4.17.11"` -> the part before the version selector.
fn yarn_name(spec: &str) -> Option<String> {
    let s = spec.trim().trim_matches('"');
    let scope_offset = usize::from(s.starts_with('@'));
    let at = s[scope_offset..].find('@').map(|i| i + scope_offset)?;
    let name = &s[..at];
    (!name.is_empty()).then(|| name.to_string())
}

/// Resolved version from a yarn `version` line: classic `version "x"` or
/// Berry `version: x`.
fn yarn_version(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("version")?.trim_start();
    let rest = rest.strip_prefix(':').map(str::trim).unwrap_or(rest);
    let v = rest.trim().trim_matches('"');
    (!v.is_empty()).then(|| v.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pnpm_key_parsing() {
        assert_eq!(
            parse_pnpm_key("lodash@4.17.11"),
            Some(("lodash".into(), "4.17.11".into()))
        );
        assert_eq!(
            parse_pnpm_key("/@babel/core@7.0.0"),
            Some(("@babel/core".into(), "7.0.0".into()))
        );
        assert_eq!(
            parse_pnpm_key("'@babel/core@7.0.0'"),
            Some(("@babel/core".into(), "7.0.0".into()))
        );
        assert_eq!(
            parse_pnpm_key("lodash@4.17.11(react@18.0.0)"),
            Some(("lodash".into(), "4.17.11".into()))
        );
        assert_eq!(
            parse_pnpm_key("@babel/core/7.0.0"),
            Some(("@babel/core".into(), "7.0.0".into()))
        );
        assert_eq!(parse_pnpm_key("some-key-no-version"), None);
    }

    #[test]
    fn yarn_name_and_version_parsing() {
        assert_eq!(yarn_name("lodash@^4.17.11").as_deref(), Some("lodash"));
        assert_eq!(yarn_name("\"@babel/core@^7.0.0\"").as_deref(), Some("@babel/core"));
        assert_eq!(yarn_name("\"lodash@npm:^4.17.11\"").as_deref(), Some("lodash"));
        assert_eq!(yarn_version("version \"4.17.21\"").as_deref(), Some("4.17.21"));
        assert_eq!(yarn_version("version: 4.17.21").as_deref(), Some("4.17.21"));
        assert_eq!(yarn_version("resolved \"https://x\""), None);
    }

    #[test]
    fn clean_semver_pins_ranges_and_rejects_unpinnable() {
        assert_eq!(clean_semver("^4.18.2").as_deref(), Some("4.18.2"));
        assert_eq!(clean_semver("~1.2.3").as_deref(), Some("1.2.3"));
        assert_eq!(clean_semver(">=2.0.0").as_deref(), Some("2.0.0"));
        assert_eq!(clean_semver("1.0.0").as_deref(), Some("1.0.0"));
        assert_eq!(clean_semver("*"), None);
        assert_eq!(clean_semver("latest"), None);
        assert_eq!(clean_semver("workspace:*"), None);
        // A bare major (no dot) is not a usable exact version.
        assert_eq!(clean_semver("7"), None);
    }
}
