//! Source scanning for the gate: test suites of a crate, the `#[test]` fns of a file with their line
//! spans, and the "Gate map" paragraph of a suite header.
//! Provenance: our own build law (AGENTS.md "Testing", gate maps of M4.4), ported from scripts/gate.ps1 (#748).

use std::path::{Path, PathBuf};

use regex::Regex;

use crate::braces::{self, Lex};
use crate::plan::ps_cmp;

/// A `#[test]` fn: its name, its 1-based line span (doc/attr lines through the closing brace) and
/// whether it carries `#[ignore]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestFn {
    pub name: String,
    pub start: usize,
    pub end: usize,
    pub ignored: bool,
}

/// Compiles a gate pattern; a bad one is an error by name, never a panic.
pub fn re(pat: &str) -> Result<Regex, String> {
    Regex::new(pat).map_err(|e| format!("bad gate pattern {pat:?}: {e}"))
}

pub fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// The integration-test suites of `tests/*.rs` and `tests/<suite>/main.rs`, by name, in gate order.
pub fn binaries(root: &Path, dir: &str) -> Vec<String> {
    let mut out: Vec<String> = rs_files(&root.join("crates").join(dir).join("tests"))
        .iter()
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    let tests = root.join("crates").join(dir).join("tests");
    if let Ok(entries) = std::fs::read_dir(&tests) {
        for entry in entries.flatten() {
            if entry.path().is_dir() && entry.path().join("main.rs").is_file() {
                out.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    out.sort_by(|a, b| ps_cmp(a, b));
    out.dedup();
    out
}

/// The `.rs` files directly in `dir`, sorted; empty when it does not exist.
pub fn rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "rs"))
            .collect(),
        Err(_) => Vec::new(),
    };
    out.sort_by(|a, b| ps_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
    out
}

/// The `.rs` files under `dir`, a directory's own files before its subdirectories (each sorted).
pub fn rs_files_recursive(dir: &Path) -> Vec<PathBuf> {
    let mut out = rs_files(dir);
    let mut subs: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect(),
        Err(_) => Vec::new(),
    };
    subs.sort_by(|a, b| ps_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
    for s in subs {
        out.extend(rs_files_recursive(&s));
    }
    out
}

/// The `#[test]` fns of a source text.
pub fn test_fns(text: &str) -> Result<Vec<TestFn>, String> {
    let test_attr = re(r"(?i)^\s*(?:#\[[^\]]*\]\s*)*#\[test\]")?;
    let ignore_attr = re(r"(?i)^\s*(?:#\[[^\]]*\]\s*)*#\[ignore(?:\s*=|\s*\])")?;
    let lead = re(r"(?i)^\s*(///|#\[)")?;
    let fn_re = re(r"(?i)^\s*(?:#\[[^\]]*\]\s*)*(pub\s+)?(async\s+)?fn\s+(\w+)")?;
    let lines: Vec<&str> = text.lines().collect();
    let n = lines.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        if !test_attr.is_match(lines[i]) {
            i += 1;
            continue;
        }
        let mut s = i;
        while s > 0 && lead.is_match(lines[s - 1]) {
            s -= 1;
        }
        let mut ignored = false;
        let mut k = s;
        let mut name = None;
        while k < n {
            if ignore_attr.is_match(lines[k]) {
                ignored = true;
            }
            if let Some(c) = fn_re.captures(lines[k]) {
                name = c.get(3).map(|m| m.as_str().to_string());
                break;
            }
            k += 1;
        }
        let Some(name) = name else { break };
        let (mut depth, mut opened, mut e) = (0i64, false, k);
        let mut lex = Lex::default();
        while e < n {
            let (o, c) = braces::count(lines[e], &mut lex);
            opened |= o > 0;
            depth += o - c;
            if opened && depth <= 0 {
                break;
            }
            e += 1;
        }
        out.push(TestFn {
            name,
            start: s + 1,
            end: e + 1,
            ignored,
        });
        i = e + 1;
    }
    Ok(out)
}

/// The "Gate map" paragraph of a suite header (first 150 lines), as one string; empty when absent.
pub fn gate_map(text: &str) -> Result<String, String> {
    let start = re(r"(?i)Gate map")?;
    let blank = re(r"^//!\s*$")?;
    let mut on = false;
    let mut acc: Vec<&str> = Vec::new();
    for l in text.lines().take(150) {
        if !on && start.is_match(l) {
            on = true;
        }
        if on {
            if !l.starts_with("//!") || blank.is_match(l) {
                break;
            }
            acc.push(l);
        }
    }
    Ok(acc.join(" "))
}
