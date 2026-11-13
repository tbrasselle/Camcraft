//! `cargo xtask release --version X.Y.Z --no-tag`: the release cut's ten phases (preflight, gate,
//! version bump, commit, release build, version check, signing, zip / packages, installer, summary),
//! one implementation on Windows, macOS and Linux. Signing and the Inno Setup installer stay in
//! scripts/sign.ps1 / installer.ps1 (Windows); macOS, Linux and FreeBSD hand off to package-*.sh.
//! Provenance: our own release law (AGENTS.md "A cut goes PR review -> merge to main -> tag"), ported
//! from scripts/release.ps1 (#749).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::decisions::valid_version;
use crate::exec::{self, git};
use crate::package::{self, PackageArgs};
use crate::release_bump as bump;
use crate::release_pack::{self as pack, Os};

pub const CO_AUTHOR: &str = "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>";

/// Parsed `release` options (release.ps1's switches, in kebab case).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReleaseArgs {
    pub version: String,
    pub title: String,
    pub no_tag: bool,
    pub skip_gate: bool,
    pub skip_build: bool,
    pub installer: bool,
    pub dry: bool,
}

pub fn parse(args: &[String]) -> Result<ReleaseArgs, String> {
    let mut r = ReleaseArgs { title: "release".into(), ..ReleaseArgs::default() };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--version" => r.version = it.next().cloned().ok_or("--version needs X.Y.Z")?,
            "--title" => r.title = it.next().cloned().ok_or("--title needs a value")?,
            "--no-tag" => r.no_tag = true,
            "--skip-gate" => r.skip_gate = true,
            "--skip-build" => r.skip_build = true,
            "--installer" => r.installer = true,
            "--dry-run" => r.dry = true,
            s => return Err(format!("unknown release option '{s}'")),
        }
    }
    if r.version.is_empty() {
        return Err("release needs --version X.Y.Z".into());
    }
    Ok(r)
}

/// One cut: its options, the host and every line it printed (the tests read them back).
pub struct Cut {
    pub root: PathBuf,
    pub a: ReleaseArgs,
    pub os: Os,
    pub out: Vec<String>,
}

impl Cut {
    pub fn new(root: &Path, a: ReleaseArgs, os: Os) -> Cut {
        Cut { root: root.to_path_buf(), a, os, out: Vec::new() }
    }

    fn say(&mut self, line: impl Into<String>) {
        let line = line.into();
        println!("{line}");
        self.out.push(line);
    }

    fn banner(&mut self, n: u8, text: &str) {
        self.say("");
        self.say(format!("=== Phase {n}/10: {text} ==="));
    }

    /// Runs a command at the root (or `cwd`); a dry run prints it instead. Non-zero exit fails the cut.
    fn run(&mut self, exe: &str, args: &[String], env: &[(&str, &Path)]) -> Result<(), String> {
        self.run_clearing(exe, args, env, &[])
    }

    /// `run`, with `clear` removed from the child's environment.
    fn run_clearing(&mut self, exe: &str, args: &[String], env: &[(&str, &Path)], clear: &[&str]) -> Result<(), String> {
        let line = format!("{exe} {}", args.join(" "));
        if self.a.dry {
            self.say(format!("[dry-run] {line}"));
            return Ok(());
        }
        self.say(format!("> {line}"));
        let program = if exe == "cargo" { exec::cargo() } else { exe.to_string() };
        let mut c = Command::new(program);
        c.args(args).current_dir(&self.root).stdin(Stdio::null());
        for k in clear {
            c.env_remove(k);
        }
        for (k, v) in env {
            c.env(k, v);
        }
        let st = c.status().map_err(|e| format!("'{line}': {e}"))?;
        match st.code() {
            Some(0) => Ok(()),
            code => Err(format!("'{line}' exited with {}", code.map_or("a signal".into(), |c| c.to_string()))),
        }
    }

    fn powershell(&mut self, script: &str, rest: &[String]) -> Result<(), String> {
        let path = self.root.join("scripts").join(script);
        let mut a: Vec<String> = ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"].map(String::from).to_vec();
        a.push(path.display().to_string());
        a.extend(rest.iter().cloned());
        self.run("powershell", &a, &[])
    }

    fn dist(&self) -> PathBuf {
        self.root.join("dist")
    }

    /// All ten phases; `Err` is the refusal printed as `RELEASE FAILED`.
    pub fn phases(&mut self) -> Result<(), String> {
        let v = self.a.version.clone();
        if !valid_version(&v) {
            return Err(format!("--version must be X.Y.Z, got '{v}'"));
        }
        if self.a.installer && self.os != Os::Windows {
            return Err(format!(
                "--installer builds the Inno Setup installer (scripts/installer.ps1), Windows only; not on {}",
                self.os.name()
            ));
        }
        if self.os == Os::Other {
            return Err(format!("no release packaging for {}: windows, linux, macos and freebsd only", self.os.name()));
        }
        if self.a.dry {
            self.say("DRY RUN: nothing below changes state.");
        }
        self.preflight(&v)?;
        self.gate()?;
        self.bump(&v)?;
        self.commit(&v)?;
        self.build()?;
        self.verify(&v)?;
        self.sign()?;
        self.package(&v)?;
        self.installer(&v)?;
        self.summary(&v)
    }

    fn preflight(&mut self, v: &str) -> Result<(), String> {
        self.banner(1, "preflight");
        let dirty = bump::unexpected_changes(&git(&self.root, &["status", "--porcelain"])?);
        if dirty.is_empty() {
            self.say("tree clean (ignoring release files)");
        } else {
            self.say("Unexpected changes:");
            for d in &dirty {
                self.say(format!("  {d}"));
            }
            if !self.a.dry {
                return Err("working tree not clean".into());
            }
            self.say("[dry-run] would fail here");
        }
        let branch = git(&self.root, &["rev-parse", "--abbrev-ref", "HEAD"])?.trim().to_string();
        // Branch/PR flow: the bump is committed on release/vX.Y.Z with --no-tag, never on main; main is
        // branch-protected, so a tagging run (the legacy direct cut) has nowhere left to run.
        if self.a.no_tag && branch == "main" {
            return Err(format!("--no-tag runs on a release/v{v} branch, not on main"));
        }
        if !self.a.no_tag {
            return Err(format!(
                "main is branch-protected (PR only): cut on release/v{v} with --no-tag and tag main after the release PR merges"
            ));
        }
        if !git(&self.root, &["tag", "--list", &format!("v{v}")])?.trim().is_empty() {
            return Err(format!("tag v{v} already exists"));
        }
        let notes = std::fs::read_to_string(self.root.join("RELEASE_NOTES.md"))
            .map_err(|e| format!("RELEASE_NOTES.md: {e}"))?;
        let n = bump::check_notes(&notes, v)?;
        self.say(format!("RELEASE_NOTES.md: {v} section ({n} lines) above the history"));
        self.say(format!("tag v{v} is free"));
        Ok(())
    }

    /// The release gate tier, run in this process (not as a nested `cargo xtask gate release`, which
    /// would make cargo rebuild the xtask exe that is running - locked on Windows).
    fn gate(&mut self) -> Result<(), String> {
        self.banner(2, "gate");
        if self.a.skip_gate {
            self.say("skipped (--skip-gate)");
            return Ok(());
        }
        self.say("cargo xtask gate release (in process)");
        let mut run = exec::Run::new(&self.root, "release", self.a.dry)?;
        run.plan(&crate::plan::release(&self.root))?;
        if run.summary() != 0 {
            return Err("the release gate failed (logs under gate-logs/)".into());
        }
        Ok(())
    }

    fn bump(&mut self, v: &str) -> Result<(), String> {
        self.banner(3, "version bump");
        let toml_path = self.root.join("Cargo.toml");
        let toml = std::fs::read_to_string(&toml_path).map_err(|e| format!("Cargo.toml: {e}"))?;
        let (old, new_toml) = bump::bump_toml(&toml, v)?;
        self.say(format!("workspace version: {old} -> {v}"));
        if self.a.dry {
            self.say("[dry-run] would rewrite Cargo.toml version and the workspace packages in Cargo.lock");
            self.say("[dry-run] cargo update -w -q");
            self.say("[dry-run] would assert Cargo.lock diff touches only version lines");
            return Ok(());
        }
        std::fs::write(&toml_path, new_toml).map_err(|e| format!("write Cargo.toml: {e}"))?;
        let lock_path = self.root.join("Cargo.lock");
        let lock = std::fs::read_to_string(&lock_path).map_err(|e| format!("Cargo.lock: {e}"))?;
        let (new_lock, n) = bump::bump_lock(&lock, &old, v);
        std::fs::write(&lock_path, new_lock).map_err(|e| format!("write Cargo.lock: {e}"))?;
        self.say(format!("Cargo.lock: {n} workspace package(s) moved to {v}"));
        self.run("cargo", &["update".into(), "-w".into(), "-q".into()], &[])?;
        let lines = bump::lock_diff_versions_only(&git(&self.root, &["diff", "Cargo.lock"])?)?;
        self.say(format!("Cargo.lock: {lines} version lines changed, nothing else"));
        Ok(())
    }

    fn commit(&mut self, v: &str) -> Result<(), String> {
        self.banner(4, "commit and tag");
        let title = self.a.title.clone();
        if self.a.dry {
            self.say("[dry-run] git add Cargo.toml RELEASE_NOTES.md TESTING.md RECOMMENDED.md (if changed)");
            self.say(format!("[dry-run] git commit -m 'chore(release): {v} - {title}' (+ Co-Authored-By)"));
            self.say(format!("[dry-run] git add Cargo.lock; git commit -m 'chore(release): sync Cargo.lock to {v}'"));
            self.say("[dry-run] no tag (--no-tag): tag main after the release PR merges");
            return Ok(());
        }
        for f in ["Cargo.toml", "RELEASE_NOTES.md", "TESTING.md", "RECOMMENDED.md"] {
            if !git(&self.root, &["status", "--porcelain", "--", f])?.trim().is_empty() {
                git(&self.root, &["add", "--", f])?;
            }
        }
        let s = |x: &str| x.to_string();
        self.run("git", &[s("commit"), s("-m"), format!("chore(release): {v} - {title}\n\n{CO_AUTHOR}")], &[])?;
        self.run("git", &[s("add"), s("Cargo.lock")], &[])?;
        self.run("git", &[s("commit"), s("-m"), format!("chore(release): sync Cargo.lock to {v}\n\n{CO_AUTHOR}")], &[])?;
        self.say("no tag (--no-tag): tag main after the release PR merges");
        Ok(())
    }

    fn build(&mut self) -> Result<(), String> {
        self.banner(5, "release build");
        if self.a.skip_build {
            self.say("skipped (--skip-build)");
            return Ok(());
        }
        // The shipped binary never depends on a compile cache: drop any compiler wrapper the environment sets
        // (a maintainer may set RUSTC_WRAPPER machine-wide). The release GATE tier may still use sccache.
        let clear = exec::packaging_env_clear(|v| std::env::var_os(v).is_some());
        for args in pack::build_steps(self.os) {
            self.run_clearing("cargo", &args, &[], &clear)?;
        }
        Ok(())
    }

    fn verify(&mut self, v: &str) -> Result<(), String> {
        let post = pack::bin_dir(&self.root, self.os).join(format!("post{}", self.os.exe()));
        self.banner(6, &format!("verify post{} version", self.os.exe()));
        if self.a.dry {
            self.say(format!("[dry-run] {} --version must report {v}", post.display()));
            return Ok(());
        }
        if !post.is_file() {
            return Err(format!("{} missing (run without --skip-build)", post.display()));
        }
        let out = Command::new(&post).arg("--version").output().map_err(|e| format!("{}: {e}", post.display()))?;
        if !out.status.success() {
            return Err(format!("{} --version failed", post.display()));
        }
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let reported = text.split_whitespace().nth(1).unwrap_or_default().to_string();
        if reported != v {
            return Err(format!("post reports '{reported}', expected {v}"));
        }
        self.say(format!("post reports {reported}"));
        Ok(())
    }

    /// Before staging, so the zip and the installer carry the same signed exes. sign.ps1 is a named
    /// no-op without a CAMCRAFT_SIGN_* certificate: an unsigned cut is still a cut.
    fn sign(&mut self) -> Result<(), String> {
        self.banner(7, "sign binaries");
        if self.os != Os::Windows {
            self.say(format!(
                "skipped on {}: sign.ps1 is Windows Authenticode (package-macos.sh codesigns its own bundle)",
                self.os.name()
            ));
            return Ok(());
        }
        let bin = pack::bin_dir(&self.root, self.os);
        let exes: Vec<String> = pack::EXES.iter().map(|e| bin.join(format!("{e}.exe")).display().to_string()).collect();
        if self.a.skip_build && !self.a.dry && !exes.first().is_some_and(|e| Path::new(e).is_file()) {
            self.say("skipped (no release exes; --skip-build)");
            return Ok(());
        }
        let mut rest = vec!["-Path".to_string()];
        rest.extend(exes);
        self.powershell("sign.ps1", &rest)
    }

    fn package(&mut self, v: &str) -> Result<(), String> {
        let bin = pack::bin_dir(&self.root, self.os);
        let Some(script) = self.os.packager() else {
            // The same staging and zip as `cargo xtask package` and release.yml's Windows job (#752).
            self.banner(8, "assemble zip");
            let a = PackageArgs { version: Some(v.to_string()), out: Some(self.dist()), bin_dir: Some(bin), dry: self.a.dry };
            let mut lines = Vec::new();
            let packed = package::package(&self.root, &a, self.os, &mut |l| lines.push(l));
            for l in lines {
                self.say(l);
            }
            return packed.map(|_| ());
        };
        self.banner(8, &format!("package ({script})"));
        let bash_args = [self.root.join(script).display().to_string()];
        self.run("bash", &bash_args, &[("CAMCRAFT_BIN_DIR", &bin)])
    }

    /// Opt-in, Windows only (refused by name elsewhere before phase 1): the same staged tree as the zip.
    fn installer(&mut self, v: &str) -> Result<(), String> {
        self.banner(9, "installer");
        let setup = self.dist().join(format!("{}-setup.exe", pack::stage_name(v)));
        if !self.a.installer {
            let why = if self.os == Os::Windows {
                format!("pass --installer to build {} with Inno Setup", pack::stage_name(v) + "-setup.exe")
            } else {
                "the Inno Setup installer is Windows only".into()
            };
            self.say(format!("skipped ({why})"));
            return Ok(());
        }
        let stage = self.dist().join(pack::stage_name(v)).display().to_string();
        let dist = self.dist().display().to_string();
        let rest = ["-Version".to_string(), v.to_string(), "-Stage".into(), stage, "-OutDir".into(), dist];
        self.powershell("installer.ps1", &rest)?;
        // installer.ps1 signs the setup exe and its embedded uninstaller through ISCC's SignTool when a
        // certificate is configured (sign.ps1 decides): no separate signing pass here.
        if self.a.dry {
            self.say("[dry-run] installer.ps1 signs setup + uninstaller via ISCC SignTool if CAMCRAFT_SIGN_* is set");
        } else if !setup.is_file() {
            self.say("no installer produced (ISCC.exe missing); the zip is the release");
        }
        Ok(())
    }

    fn summary(&mut self, v: &str) -> Result<(), String> {
        self.banner(10, "done");
        if self.os == Os::Windows {
            let zip = self.dist().join(format!("{}.zip", pack::stage_name(v)));
            self.say(format!("Zip: {}", zip.display()));
            let setup = self.dist().join(format!("{}-setup.exe", pack::stage_name(v)));
            if self.a.installer && setup.is_file() {
                self.say(format!("Installer: {}", setup.display()));
            }
        } else {
            self.say(format!("Packages: {}", self.dist().display()));
        }
        let branch = git(&self.root, &["rev-parse", "--abbrev-ref", "HEAD"])?.trim().to_string();
        self.say(format!("Next: git push -u origin {branch}; gh pr create --base main --title 'chore(release): {v}'"));
        self.say(format!("      record tip=$(git rev-parse {branch}) before merging; after it merges: git switch main; git pull --ff-only;"));
        self.say(format!("      git diff $tip main must be empty, then git tag -a v{v} -m 'CamCraft {v}' main; git push origin v{v}"));
        self.say("NOTHING WAS PUSHED. Pushing is left to you.");
        Ok(())
    }
}

/// `cargo xtask release`: the exit code (0 done, 1 refused or failed).
pub fn release(root: &Path, a: ReleaseArgs) -> i32 {
    let mut cut = Cut::new(root, a, Os::host());
    match cut.phases() {
        Ok(()) => 0,
        Err(e) => {
            cut.say(format!("RELEASE FAILED: {e}"));
            1
        }
    }
}
