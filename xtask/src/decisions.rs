//! `cargo xtask decisions --version X.Y.Z`: gather docs/decisions/*.md into RECOMMENDED.md at a release
//! cut (one file per PR keeps issue branches from conflicting on RECOMMENDED.md).
//! Provenance: our own process law (AGENTS.md "Every recommendation is recorded", release step 2),
//! ported from scripts/collect-decisions.ps1 (#748).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::plan::ps_cmp;

/// `X.Y.Z`, digits only.
pub fn valid_version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// The decision files in collection order: by the issue number that leads the name, then by name.
pub fn files(dir: &Path) -> Vec<PathBuf> {
    let num = |p: &PathBuf| -> u64 {
        let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        stem.split('-').next().and_then(|n| n.parse().ok()).unwrap_or(0)
    };
    let name = |p: &PathBuf| p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut v: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "md"))
            .filter(|p| !name(p).eq_ignore_ascii_case("README.md"))
            .collect(),
        Err(_) => Vec::new(),
    };
    v.sort_by(|a, b| num(a).cmp(&num(b)).then_with(|| ps_cmp(&name(a), &name(b))));
    v
}

/// The text appended to RECOMMENDED.md for these files.
pub fn section(version: &str, files: &[PathBuf]) -> Result<String, String> {
    let mut text = format!("\n## v{version} - decisions gathered at the cut\n");
    for f in files {
        let raw = std::fs::read_to_string(f).map_err(|e| format!("read {}: {e}", f.display()))?;
        let body = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
        text.push('\n');
        text.push_str(body.trim_end());
        text.push('\n');
    }
    Ok(text)
}

/// Appends the section and removes the gathered files; the line printed for the operator.
pub fn collect(root: &Path, version: &str) -> Result<String, String> {
    if !valid_version(version) {
        return Err(format!("--version must be X.Y.Z, got '{version}'"));
    }
    let list = files(&root.join("docs").join("decisions"));
    if list.is_empty() {
        return Ok("no decision files to collect".into());
    }
    let text = section(version, &list)?;
    let out = root.join("RECOMMENDED.md");
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&out)
        .map_err(|e| format!("open {}: {e}", out.display()))?;
    f.write_all(text.as_bytes()).map_err(|e| format!("write {}: {e}", out.display()))?;
    for p in &list {
        std::fs::remove_file(p).map_err(|e| format!("remove {}: {e}", p.display()))?;
    }
    Ok(format!("collected {} decision file(s) into RECOMMENDED.md", list.len()))
}
