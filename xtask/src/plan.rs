//! The gate's fixed lists and the batch / release plans (AGENTS.md "Three gate tiers").
//! Provenance: our own build law, ported from scripts/gate.ps1 (#748); drives only the public cargo / libtest CLI.

use std::cmp::Ordering;
use std::path::Path;

use crate::scan;

/// Debug test binaries over ~40 s: skipped by batch, run by release (and by quick when the diff maps to
/// them). Format `crate-dir:binary`. Measured 2026-11-07 (debug, -j 6, 6 test threads).
pub const SLOW_BINARIES: &[&str] = &[
    "motion:finish3d",          // ~50 s
    "motion:pivot5d",           // ~46 s
    "motion:flow",              // ~42 s
    "app:center_cut_multiaxis", // ~40 s
];
/// Crate dirs in dependency order. kernel builds a C chain (truck dev-dep) for its own tests: release
/// tier and quick-when-touched only.
pub const BATCH_CRATES: &[&str] = &["geometry", "cl", "tools", "post", "sim", "motion", "app"];
pub const ALL_CRATES: &[&str] = &[
    "geometry", "kernel", "cl", "tools", "post", "sim", "motion", "app", "ui",
];
/// A touch to a shared crate (or the workspace manifest) makes quick a batch.
pub const SHARED: &[&str] = &["geometry", "cl"];
pub const CLIPPY: &[&str] = &["geometry", "cl", "tools", "post", "sim", "motion", "app", "ui"];
pub const J: &str = "6";
pub const T: &str = "--test-threads=6";

/// One cargo command of a plan. `in_ui` runs it with CWD `crates/ui` (the snapshot goldens resolve
/// from there); every other step runs at the workspace root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub name: String,
    pub args: Vec<String>,
    pub in_ui: bool,
}

impl Step {
    pub fn new(name: impl Into<String>, args: &[&str], dir: &str) -> Step {
        Step {
            name: name.into(),
            args: args.iter().map(|a| (*a).to_string()).collect(),
            in_ui: dir == "ui",
        }
    }
    pub fn with(mut self, more: &[String]) -> Step {
        self.args.extend(more.iter().cloned());
        self
    }
}

/// A plan: the lines it prints first, then its steps.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Plan {
    pub notes: Vec<String>,
    pub steps: Vec<Step>,
}

pub fn pkg(dir: &str) -> String {
    format!("camcraft-{dir}")
}

pub fn is_slow(dir: &str, bin: &str) -> bool {
    let key = format!("{dir}:{bin}");
    SLOW_BINARIES.contains(&key.as_str())
}

/// PowerShell's `Sort-Object` order for the ASCII names the gate sorts (crate dirs, suite names, fn
/// names): case-insensitive, punctuation before digits before letters. Keeps step order identical to
/// what gate.ps1 printed.
pub fn ps_cmp(a: &str, b: &str) -> Ordering {
    fn key(c: char) -> (u8, char) {
        let class = if c.is_alphabetic() {
            2
        } else if c.is_numeric() {
            1
        } else {
            0
        };
        (class, c.to_ascii_lowercase())
    }
    a.chars().map(key).cmp(b.chars().map(key)).then_with(|| a.cmp(b))
}

pub fn ps_sorted<I: IntoIterator<Item = String>>(items: I) -> Vec<String> {
    let mut v: Vec<String> = items.into_iter().collect();
    v.sort_by(|a, b| ps_cmp(a, b));
    v.dedup();
    v
}

fn strs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

/// Every suite of `dirs`; with `skip_slow` a crate runs `--lib` plus each non-slow binary by name.
pub fn run_suites(root: &Path, dirs: &[&str], skip_slow: bool) -> Vec<Step> {
    let mut out = Vec::new();
    for &d in dirs {
        let p = pkg(d);
        if d == "ui" {
            out.push(Step::new(
                "ui suites (cwd crates/ui)",
                &["test", "-j", J, "-p", &p, "--no-fail-fast", "--", T],
                "ui",
            ));
            continue;
        }
        if !skip_slow {
            out.push(Step::new(
                format!("{d} suites"),
                &["test", "-j", J, "-p", &p, "--no-fail-fast", "--", T],
                "",
            ));
            continue;
        }
        let mut sel = vec!["--lib".to_string()];
        let mut skipped = Vec::new();
        for b in scan::binaries(root, d) {
            if is_slow(d, &b) {
                skipped.push(b);
            } else {
                sel.push("--test".into());
                sel.push(b);
            }
        }
        let name = if skipped.is_empty() {
            format!("{d} suites")
        } else {
            format!("{d} suites (slow, release tier: {})", skipped.join(", "))
        };
        let mut args = vec!["test", "-j", J, "-p", &p, "--no-fail-fast"];
        args.extend(strs(&sel));
        args.extend(["--", T]);
        out.push(Step::new(name, &args, ""));
    }
    out
}

pub fn clippy() -> Step {
    let mut args: Vec<String> = vec!["clippy".into(), "-j".into(), J.into()];
    for d in CLIPPY {
        args.push("-p".into());
        args.push(pkg(d));
    }
    for a in ["--all-targets", "--", "-D", "warnings"] {
        args.push(a.into());
    }
    Step::new("clippy -D warnings", &[], "").with(&args)
}

pub fn batch(root: &Path) -> Plan {
    let mut plan = Plan::default();
    plan.notes.push(format!(
        "batch: debug suites minus SLOW_BINARIES ({}), ui, clippy",
        SLOW_BINARIES.join(", ")
    ));
    plan.steps.extend(run_suites(root, BATCH_CRATES, true));
    plan.steps.extend(run_suites(root, &["ui"], false));
    plan.steps.push(clippy());
    plan
}

pub fn release(root: &Path) -> Plan {
    release_with(root, true)
}

/// The release plan; `snapshots: false` (`gate release --no-snapshots`, #1413) leaves out the two ui
/// pixel-snapshot lanes, whose goldens are made on Windows: the cut workflow's Linux runner runs every
/// other lane, and the snapshot lanes stay a local step at the cut.
pub fn release_with(root: &Path, snapshots: bool) -> Plan {
    let mut plan = Plan::default();
    plan.notes.push(
        "release: every debug suite, ui, clippy, then the --ignored heavy lanes in release and the ui snapshot lane"
            .into(),
    );
    if !snapshots {
        plan.notes.push(
            "--no-snapshots: the ui pixel-snapshot lanes are skipped (Windows-made goldens); run them locally at the cut".into(),
        );
    }
    let non_ui: Vec<&str> = ALL_CRATES.iter().copied().filter(|d| *d != "ui").collect();
    plan.steps.extend(run_suites(root, &non_ui, false));
    plan.steps.extend(run_suites(root, &["ui"], false));
    plan.steps.push(clippy());
    for d in ["motion", "app"] {
        let p = pkg(d);
        plan.steps.push(Step::new(
            format!("{d} heavy lane (release --ignored)"),
            &["test", "--release", "-j", J, "-p", &p, "--no-fail-fast", "--", T, "--ignored"],
            "",
        ));
    }
    if !snapshots {
        return plan;
    }
    // The ui pixel snapshot goldens (#255): #[ignore]d out of quick/batch, run here only, debug like the
    // goldens were made, CWD crates/ui so they resolve tests/snapshots/. Regenerated at the cut with
    // UPDATE_SNAPSHOTS=1.
    let p = pkg("ui");
    plan.steps.push(Step::new(
        "ui snapshot lane (debug --ignored, cwd crates/ui)",
        &["test", "-j", J, "-p", &p, "--lib", "--no-fail-fast", "--", T, "--ignored", "snapshot"],
        "ui",
    ));
    // The docs-picture goldens (#525, #854) live in the gpu_render integration binary, which `--lib`
    // above does not reach: run its `--ignored snapshot` test(s) here too.
    plan.steps.push(Step::new(
        "ui gpu_render snapshot lane (debug --ignored)",
        &["test", "-j", J, "-p", &p, "--test", "gpu_render", "--no-fail-fast", "--", T, "--ignored", "snapshot"],
        "ui",
    ));
    plan
}
