//! The quick tier's plan: from the files a diff touches to the test fns and suites that cover them.
//! Provenance: our own build law (AGENTS.md "Three gate tiers", "Test the change"), ported from
//! scripts/gate.ps1 (#748): changed #[test] fns run --exact, a changed src module runs the suites whose
//! Gate map names it, shared test code runs the crate's suites, shared crates / the manifest -> batch.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use regex::escape;

use crate::assets;
use crate::path_mods;
use crate::plan::{ALL_CRATES, J, Plan, SHARED, Step, T, pkg, ps_cmp, ps_sorted};
use crate::scan::{self, re};
use crate::suites;

/// What quick does with a diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Quick {
    /// A shared crate or the workspace manifest changed (the first such file): run batch instead.
    Batch(String),
    Plan(Plan),
}

/// The diff, as quick reads it.
pub struct Diff<'a> {
    /// Changed and untracked files, `/`-separated, relative to the root.
    pub changed: &'a [String],
    pub untracked: &'a [String],
    /// The `@@ ` hunk headers of one file's zero-context diff against the merge base.
    pub hunks: &'a dyn Fn(&str) -> Vec<String>,
    /// Source at the merge base; an unavailable preimage requires conservative suite selection.
    pub before: &'a dyn Fn(&str) -> Option<String>,
}

#[derive(Default)]
struct Picks {
    whole: BTreeSet<String>,
    why: BTreeMap<String, String>,
    fns: BTreeSet<(String, String)>,
    ign: BTreeSet<(String, String)>,
    libs: BTreeSet<String>,
    mods: Vec<(String, String, String)>,
}

pub fn plan(root: &Path, diff: &Diff, heavy: bool) -> Result<Quick, String> {
    let manifest = re(r"(?i)^(Cargo\.(toml|lock)$|\.cargo/|rust-toolchain)")?;
    let shared_crate = re(&format!("(?i)^crates/({})/", SHARED.join("|")))?;
    if let Some(f) = diff
        .changed
        .iter()
        .find(|f| manifest.is_match(f) || shared_crate.is_match(f))
    {
        return Ok(Quick::Batch(f.clone()));
    }
    let mut p = Picks::default();
    let mut plan = Plan::default();
    let in_crate = re(r"(?i)^crates/([^/]+)/(.+)$")?;
    let suite_file = re(r"(?i)^tests/([^/]+?)(?:\.rs$|/)")?;
    let src_file = re(r"(?i)^src/([^/]+?)(\.rs)?(/([^/]+?)(\.rs)?(/.*)?)?$")?;
    let hunk_re = re(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")?;
    let xtask = diff.changed.iter().any(|f| f.starts_with("xtask/"));
    for f in diff.changed {
        let Some(c) = in_crate.captures(f) else {
            continue;
        };
        let (d, rest) = (c[1].to_string(), c[2].to_string());
        if !ALL_CRATES.contains(&d.as_str()) {
            continue;
        }
        let abs = root.join(f);
        if rest.starts_with("tests/")
            && rest.ends_with(".rs")
            && rest.matches('/').count() == 1
            && !abs.is_file()
        {
            continue; // deleted flat suite: Cargo no longer has this target
        }
        if let Some(t) = suite_file
            .captures(&rest)
            .filter(|t| suites::entry(root, &d, &t[1]).is_file())
        {
            let key = format!("{d}:{}", &t[1]);
            if !abs.is_file() {
                p.whole.insert(key.clone());
                p.why.insert(key, "suite module deleted".into());
                continue;
            }
            if diff.untracked.iter().any(|u| u.eq_ignore_ascii_case(f)) {
                p.whole.insert(key.clone());
                p.why.insert(key, "new suite".into());
                continue;
            }
            let sources = suites::sources(&suites::entry(root, &d, &t[1]))?;
            let source = sources.iter().find(|s| s.path == abs);
            let all = match source {
                Some(s) if s.precise => suites::test_fns(s)?,
                _ => Vec::new(),
            };
            let before = (diff.before)(f)
                .map(|text| scan::test_fns(&text))
                .transpose()?;
            let mut outside = source.is_none_or(|s| !s.precise) || before.is_none();
            for hk in (diff.hunks)(f) {
                let Some(h) = hunk_re.captures(&hk) else {
                    continue;
                };
                let old_start: usize = h[1].parse().unwrap_or(0);
                let old_count: usize = h.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(1);
                let s: usize = h[3].parse().unwrap_or(0);
                let c: usize = h.get(4).and_then(|m| m.as_str().parse().ok()).unwrap_or(1);
                let e = s.max(s.saturating_add(c).saturating_sub(1));
                let lo = s.max(1);
                let hit: Vec<_> = all.iter().filter(|t| t.start <= e && t.end >= lo).collect();
                if c == 0
                    || (lo..=e).any(|line| !all.iter().any(|t| t.start <= line && t.end >= line))
                    || before.as_ref().is_some_and(|tests| {
                        (old_start..old_start.saturating_add(old_count))
                            .any(|line| !tests.iter().any(|t| t.start <= line && t.end >= line))
                    })
                {
                    outside = true;
                }
                for t in hit {
                    let set = if t.ignored { &mut p.ign } else { &mut p.fns };
                    set.insert((key.clone(), t.name.clone()));
                }
            }
            if outside {
                p.whole.insert(key.clone());
                p.why
                    .insert(key, "suite changed outside its test fns".into());
            }
        } else if rest.to_ascii_lowercase().starts_with("tests/")
            || rest.eq_ignore_ascii_case("Cargo.toml")
            || rest.eq_ignore_ascii_case("build.rs")
        {
            for b in scan::binaries(root, &d) {
                let key = format!("{d}:{b}");
                p.whole.insert(key.clone());
                p.why.insert(key, format!("shared test input {rest}"));
            }
            p.libs.insert(d);
        } else if let Some(s) = src_file.captures(&rest) {
            p.libs.insert(d.clone());
            let m = s[1].to_string();
            let sub = match s.get(4).map(|x| x.as_str()) {
                Some(x) if x != "mod" && x != "tests" => x.to_string(),
                _ => String::new(),
            };
            // #1021: a `#[path]`-included file is also its parent's submodule; both names are
            // mapped (a union), so resolving the includer can only ever add suites.
            if let Some((pm, ps)) = path_mods::included_as(root, &d, &rest)?
                && (pm.as_str(), ps.as_str()) != (m.as_str(), sub.as_str())
            {
                p.mods.push((d.clone(), pm, ps));
            }
            if !["lib", "main", "bin"].contains(&m.as_str()) {
                p.mods.push((d, m, sub));
            }
        }
    }
    p.mods.sort_by(|a, b| {
        ps_cmp(&a.0, &b.0)
            .then_with(|| ps_cmp(&a.1, &b.1))
            .then_with(|| ps_cmp(&a.2, &b.2))
    });
    p.mods.dedup();
    map_modules(root, &mut p, &mut plan.notes)?;
    let unmapped = map_assets(root, diff.changed, &mut p)?;
    describe(&p, heavy, xtask, &unmapped, &mut plan.notes);
    plan.steps = steps(p, heavy, xtask);
    Ok(Quick::Plan(plan))
}

/// src modules -> the suites whose Gate map names them (own crate: `::mod` or `{..mod..}`; others:
/// `crate::mod`). A file in a submodule picks the suites naming `mod::sub` or `mod (.. sub ..)` when
/// there are any, else every suite naming the module.
fn map_modules(root: &Path, p: &mut Picks, notes: &mut Vec<String>) -> Result<(), String> {
    if p.mods.is_empty() {
        return Ok(());
    }
    let mut maps: Vec<(String, String, String)> = Vec::new();
    for d in ALL_CRATES {
        for b in scan::binaries(root, d) {
            let mut gm = String::new();
            for source in suites::sources(&suites::entry(root, d, &b))? {
                gm.push_str(&scan::gate_map(&scan::read(&source.path))?);
                gm.push(' ');
            }
            if !gm.is_empty() {
                maps.push(((*d).to_string(), b, gm));
            }
        }
    }
    for (md, m, sub) in p.mods.clone() {
        let (mm, ms) = (escape(&m), escape(&sub));
        let own = re(&format!(r"(?i)::{mm}\b|\{{[^}}]*\b{mm}\b"))?;
        let other = re(&format!(r"(?i)\b(camcraft_)?{md}::{mm}\b"))?;
        let by_sub_re = re(&format!(
            r"(?i)(::|\{{[^}}]*\b){mm}(::{ms}\b|\s*\([^)]*\b{ms}\b)"
        ))?;
        let (mut by_sub, mut by_mod) = (Vec::new(), Vec::new());
        for (d, b, gm) in &maps {
            let hit = if *d == md {
                own.is_match(gm)
            } else {
                other.is_match(gm)
            };
            if !hit {
                continue;
            }
            by_mod.push(format!("{d}:{b}"));
            if !sub.is_empty() && by_sub_re.is_match(gm) {
                by_sub.push(format!("{d}:{b}"));
            }
        }
        let use_sub = !by_sub.is_empty();
        let pick = if use_sub { by_sub } else { by_mod };
        let lbl = if use_sub {
            format!("{md}::{m}::{sub}")
        } else {
            format!("{md}::{m}")
        };
        let shown_sub = if sub.is_empty() {
            String::new()
        } else {
            format!("::{sub}")
        };
        notes.push(format!(
            "  src {md}::{m}{shown_sub} -> {} suite(s) by gate map ({})",
            pick.len(),
            if use_sub { "submodule" } else { "module" }
        ));
        for k in pick {
            p.whole.insert(k.clone());
            p.why
                .entry(k)
                .or_insert_with(|| format!("gate map names {lbl}"));
        }
    }
    Ok(())
}

/// Changed post scripts / machine files -> the suites that post through them; returns the assets no
/// suite covers (reported by `describe`, never skipped silently).
fn map_assets(root: &Path, changed: &[String], p: &mut Picks) -> Result<Vec<String>, String> {
    let mut unmapped = Vec::new();
    for a in assets::changed_assets(changed) {
        if !root.join(&a.path).is_file() {
            continue; // deleted: nothing left to post through
        }
        let hits = assets::suites_for(root, &a)?;
        if hits.is_empty() {
            unmapped.push(a.path.clone());
        }
        for (k, why) in hits {
            p.whole.insert(k.clone());
            p.why
                .entry(k)
                .or_insert_with(|| format!("{} {why}", a.path));
        }
    }
    Ok(unmapped)
}

fn by_fn(set: &BTreeSet<(String, String)>) -> Vec<(String, String)> {
    let mut v: Vec<_> = set.iter().cloned().collect();
    v.sort_by(|a, b| ps_cmp(&a.0, &b.0).then_with(|| ps_cmp(&a.1, &b.1)));
    v
}

fn describe(p: &Picks, heavy: bool, xtask: bool, unmapped: &[String], notes: &mut Vec<String>) {
    notes.push("quick plan:".into());
    for d in ps_sorted(p.libs.iter().cloned()) {
        notes.push(format!("  {d} lib (unit tests)"));
    }
    for k in ps_sorted(p.whole.iter().cloned()) {
        let why = p.why.get(&k).map(String::as_str).unwrap_or("");
        notes.push(format!("  {k}  whole suite - {why}"));
    }
    let ign_note = if heavy {
        "  [ignored -> release --ignored]"
    } else {
        "  [ignored: heavy lane, skipped - pass --heavy or use `gate test <name>`]"
    };
    let mut both: Vec<(String, String, bool)> = by_fn(&p.fns)
        .into_iter()
        .map(|(k, f)| (k, f, false))
        .collect();
    both.extend(by_fn(&p.ign).into_iter().map(|(k, f)| (k, f, true)));
    both.sort_by(|a, b| ps_cmp(&a.0, &b.0).then_with(|| ps_cmp(&a.1, &b.1)));
    for (k, f, ig) in both {
        if !p.whole.contains(&k) {
            notes.push(format!("  {k}::{f}{}", if ig { ign_note } else { "" }));
        }
    }
    if xtask {
        notes.push("  xtask (unit tests) - xtask/ changed".into());
    }
    for a in unmapped {
        notes.push(format!(
            "  {a}: NO SUITE covers this asset - nothing posts through it in crates/*/tests"
        ));
    }
    if unmapped.is_empty()
        && p.libs.is_empty()
        && p.whole.is_empty()
        && p.fns.is_empty()
        && p.ign.is_empty()
        && !xtask
    {
        notes.push("  nothing in crates/ changed: no tests to run".into());
    }
}

fn steps(mut p: Picks, heavy: bool, xtask: bool) -> Vec<Step> {
    let mut build_only: BTreeSet<String> = BTreeSet::new();
    if !heavy {
        build_only = p.ign.iter().map(|(k, _)| k.clone()).collect();
        p.ign.clear();
    }
    let whole = ps_sorted(p.whole.iter().cloned());
    let mut out = Vec::new();
    for &d in ALL_CRATES {
        let pk = pkg(d);
        let mine = |k: &String| k.split(':').next() == Some(d);
        let bin = |k: &String| k.split(':').nth(1).unwrap_or("").to_string();
        let mut sel: Vec<String> = Vec::new();
        if p.libs.contains(d) {
            sel.push("--lib".into());
        }
        for k in whole.iter().filter(|k| mine(k)) {
            sel.extend(["--test".to_string(), bin(k)]);
        }
        if !sel.is_empty() {
            let base = Step::new(
                format!("{d} suites"),
                &["test", "-j", J, "-p", &pk, "--no-fail-fast"],
                d,
            );
            out.push(base.with(&sel).with(&["--".into(), T.into()]));
        }
        // A suite whose only changed fns are skipped heavy ones still has to compile.
        let mut bo: Vec<String> = Vec::new();
        for k in ps_sorted(build_only.iter().cloned())
            .iter()
            .filter(|k| mine(k))
        {
            if !p.whole.contains(k) && !p.fns.iter().any(|(fk, _)| fk == k) {
                bo.extend(["--test".to_string(), bin(k)]);
            }
        }
        if !bo.is_empty() {
            let name = format!("{d} build-only (changed fns are heavy lane)");
            out.push(Step::new(name, &["test", "-j", J, "-p", &pk, "--no-run"], d).with(&bo));
        }
        for (set, is_ign) in [(&p.fns, false), (&p.ign, true)] {
            let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (k, f) in by_fn(set) {
                if mine(&k) && !p.whole.contains(&k) {
                    groups.entry(k).or_default().push(f);
                }
            }
            let mut keys: Vec<String> = groups.keys().cloned().collect();
            keys.sort_by(|a, b| ps_cmp(a, b));
            for k in keys {
                let names = groups.remove(&k).unwrap_or_default();
                let b = bin(&k);
                let tag = if is_ign { ", ignored, release" } else { "" };
                let name = format!("{d} {b} ({} fn{tag})", names.len());
                let mut args: Vec<String> = vec!["test".into()];
                if is_ign {
                    args.push("--release".into());
                }
                for a in ["-j", J, "-p", &pk, "--test", &b, "--", T] {
                    args.push(a.into());
                }
                if is_ign {
                    args.push("--ignored".into());
                }
                args.push("--exact".into());
                args.extend(names);
                out.push(Step::new(name, &[], d).with(&args));
            }
        }
    }
    if xtask {
        out.push(Step::new(
            "xtask unit tests",
            &["test", "-j", J, "-p", "xtask", "--", T],
            "",
        ));
    }
    out
}
