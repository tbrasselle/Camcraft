//! `cargo xtask`: the workspace's process tooling, one implementation for Windows, macOS and Linux -
//! the gate tiers (`gate quick|batch|release|test`), the decision collection (`decisions`), the icon
//! rasteriser (`icons`, #661), the Windows portable zip (`package`) and the release cut (`release`).
//! Provenance: our own build and process law (AGENTS.md "Three gate tiers", "Test the change",
//! release step 2), replacing the scripts/gate.*, collect-decisions.* and release.ps1 twins (#748, #749).

#[cfg(test)]
mod asset_tests;
mod assets;
mod braces;
mod checkout;
#[cfg(test)]
mod checkout_tests;
mod decisions;
#[cfg(test)]
mod directory_tests;
mod exec;
#[cfg(test)]
mod gate_maps_tests;
mod icon_font;
mod icons;
#[cfg(test)]
mod icons_tests;
mod logo;
#[cfg(test)]
mod logo_tests;
mod merge_delta;
mod merge_plan;
#[cfg(test)]
mod merge_plan_det_tests;
mod merge_plan_git;
mod merge_plan_out;
#[cfg(test)]
mod merge_plan_tests;
mod package;
mod path_mods;
mod plan;
mod quick;
mod release;
mod release_bump;
mod release_pack;
#[cfg(test)]
mod release_tests;
mod scan;
mod single;
mod suites;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod package_paths_tests;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
usage:
  cargo xtask gate quick [--base main] [--heavy]   tests the diff touches (test fns where it can)
  cargo xtask gate batch                           every debug suite minus SLOW_BINARIES, ui, clippy
  cargo xtask gate release [--no-snapshots]        everything, plus the --ignored heavy lanes and the ui snapshot lane
                                                   (--no-snapshots: not the Windows-made pixel goldens - cut.yml on Linux, #1413)
  cargo xtask gate test <filter> [--crate c] [--binary b] [--exact]   exactly the matching test(s)
      any gate: --dry-run (or --plan) prints the plan without running it. Logs: gate-logs/<tier>-<timestamp>/.
  cargo xtask icons [--check | --sheet <out.png> [--px N] [--zoom N]]
                                                   rasterise assets/ui/svg/*.svg to the 32 px PNGs (#661), check
                                                   them against their masters, or draw a 16 px contact sheet
  cargo xtask logo [--check]                       generate assets/logo/icons/ (PNG sizes, camcraft.ico) from the logotype (#1459)
  cargo xtask decisions --version X.Y.Z            gather docs/decisions/*.md into RECOMMENDED.md
  cargo xtask release --version X.Y.Z --no-tag [--title T] [--skip-gate] [--skip-build] [--installer] [--dry-run]
                                                   the release cut on release/vX.Y.Z (scripts/release.ps1's phases)
  cargo xtask merge-plan <PR>... | --label <name> [--json] | --delta <PR> --reviewed <sha>
                                                   the merge order of open PRs (waves, refresh only at merge time)
                                                   or the delta class of a PR since a reviewed head (#1221)
  cargo xtask package [--version X.Y.Z] [--out dist] [--bin-dir target/release] [--dry-run]
                                                   stage the Windows portable tree and write camcraft-<ver>-win-x64.zip
                                                   (no bump/commit/build; version defaults to post.exe --version;
                                                   reports version/stage/zip to $GITHUB_OUTPUT when set)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::from(2)
        }
    }
}

/// The workspace root: the parent of xtask/ (CARGO_MANIFEST_DIR at run time under `cargo xtask`, else
/// where this binary was built from).
fn root() -> PathBuf {
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    manifest.parent().map(Path::to_path_buf).unwrap_or(manifest)
}

/// Parsed `gate` options.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct GateArgs {
    pub tier: String,
    pub filter: Option<String>,
    pub base: String,
    pub krate: Option<String>,
    pub binary: Option<String>,
    pub exact: bool,
    pub heavy: bool,
    pub dry: bool,
    /// `gate release --no-snapshots` (#1413): the release plan without the two ui pixel-snapshot lanes.
    pub no_snapshots: bool,
}

pub fn parse_gate(args: &[String]) -> Result<GateArgs, String> {
    let mut g = GateArgs {
        base: "main".into(),
        ..GateArgs::default()
    };
    let mut it = args.iter();
    let value = |flag: &str, it: &mut std::slice::Iter<String>| {
        it.next()
            .cloned()
            .ok_or_else(|| format!("{flag} needs a value"))
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--base" => g.base = value(a, &mut it)?,
            "--crate" => g.krate = Some(value(a, &mut it)?),
            "--binary" => g.binary = Some(value(a, &mut it)?),
            "--exact" => g.exact = true,
            "--heavy" => g.heavy = true,
            "--dry-run" | "--plan" => g.dry = true,
            "--no-snapshots" => g.no_snapshots = true,
            s if s.starts_with('-') => return Err(format!("unknown gate option '{s}'\n{USAGE}")),
            s if g.tier.is_empty() => g.tier = s.to_string(),
            s if g.tier == "test" && g.filter.is_none() => g.filter = Some(s.to_string()),
            s => return Err(format!("unexpected argument '{s}'\n{USAGE}")),
        }
    }
    if g.tier.is_empty() {
        g.tier = "quick".into();
    }
    if g.no_snapshots && g.tier != "release" {
        return Err(format!("--no-snapshots applies to the release tier only, not '{}'", g.tier));
    }
    match g.tier.as_str() {
        "quick" | "batch" | "release" => Ok(g),
        "test" => match &g.filter {
            Some(f)
                if f.split("::").all(|p| {
                    !p.is_empty() && p.chars().all(|c| c.is_alphanumeric() || c == '_')
                }) =>
            {
                Ok(g)
            }
            Some(f) => Err(format!(
                "test filter must be a fn name or module::fn path (letters, digits, _): '{f}'"
            )),
            None => Err(format!("gate test needs a filter\n{USAGE}")),
        },
        t => Err(format!(
            "unknown tier '{t}' (quick | batch | release | test)\n{USAGE}"
        )),
    }
}

fn run(args: &[String]) -> Result<i32, String> {
    match args.first().map(String::as_str) {
        Some("gate") => gate(&root(), &parse_gate(&args[1..])?),
        Some("decisions") => {
            let version = match args.get(1..) {
                Some([flag, v]) if flag == "--version" => v.clone(),
                Some([v]) if !v.starts_with('-') => v.clone(),
                _ => return Err(format!("decisions needs --version X.Y.Z\n{USAGE}")),
            };
            println!("{}", decisions::collect(&root(), &version)?);
            Ok(0)
        }
        Some("icons") => icons::run(&root(), &args[1..]),
        Some("logo") => logo::run(&root(), &args[1..]),
        Some("merge-plan") => merge_plan_git::run(&root(), &args[1..]),
        Some("package") => {
            let a = package::parse(&args[1..]).map_err(|e| format!("{e}\n{USAGE}"))?;
            Ok(package::run(&root(), &a))
        }
        Some("release") => {
            let a = release::parse(&args[1..]).map_err(|e| format!("{e}\n{USAGE}"))?;
            Ok(release::release(&root(), a))
        }
        Some("-h" | "--help" | "help") | None => {
            println!("{USAGE}");
            Ok(0)
        }
        Some(other) => Err(format!("unknown task '{other}'\n{USAGE}")),
    }
}

fn gate(root: &Path, g: &GateArgs) -> Result<i32, String> {
    let mut run = exec::Run::new(root, &g.tier, g.dry)?;
    if let Some(name) = &g.filter {
        let f = single::Filter {
            name,
            krate: g.krate.as_deref(),
            binary: g.binary.as_deref(),
            exact: g.exact,
        };
        match single::plan(root, &f) {
            Ok(p) => run.plan(&p)?,
            Err(e) => {
                println!("{e}");
                return Ok(1);
            }
        }
        return Ok(run.summary());
    }
    let plan = match g.tier.as_str() {
        "quick" => {
            let mb = exec::git(root, &["merge-base", &g.base, "HEAD"])
                .map(|s| s.trim().to_string())
                .ok()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| format!("no merge base with '{}'", g.base))?;
            let lines = |s: String| -> Vec<String> {
                s.lines()
                    .filter(|l| !l.is_empty())
                    .map(|l| l.replace('\\', "/"))
                    .collect()
            };
            let untracked = lines(exec::git(
                root,
                &["ls-files", "--others", "--exclude-standard"],
            )?);
            let mut changed = lines(exec::git(root, &["diff", "--name-only", &mb])?);
            changed.extend(untracked.iter().cloned());
            let changed = plan::ps_sorted(changed);
            let short: String = mb.chars().take(7).collect();
            println!(
                "changed since {} ({short}): {} file(s)",
                g.base,
                changed.len()
            );
            let hunks = |f: &str| -> Vec<String> {
                exec::git(root, &["diff", "-U0", &mb, "--", f])
                    .map(|s| {
                        s.lines()
                            .filter(|l| l.starts_with("@@ "))
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let before = |f: &str| exec::git(root, &["show", &format!("{mb}:{f}")]).ok();
            let diff = quick::Diff {
                changed: &changed,
                untracked: &untracked,
                hunks: &hunks,
                before: &before,
            };
            match quick::plan(root, &diff, g.heavy)? {
                quick::Quick::Plan(p) => p,
                quick::Quick::Batch(f) => {
                    println!("shared crate / workspace touched ({f}): quick falls back to batch");
                    plan::batch(root)
                }
            }
        }
        _ => tier_plan(root, g),
    };
    run.plan(&plan)?;
    Ok(run.summary())
}

/// The plan of a non-quick tier, pure (no cargo, no git): `gate()` runs exactly this, so the
/// `--no-snapshots` wiring is pinned by `tests::gate_no_snapshots_reaches_the_release_plan`.
pub(crate) fn tier_plan(root: &Path, g: &GateArgs) -> plan::Plan {
    match g.tier.as_str() {
        "batch" => plan::batch(root),
        _ => plan::release_with(root, !g.no_snapshots),
    }
}
