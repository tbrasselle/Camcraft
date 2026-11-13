//! `cargo xtask package [--version X.Y.Z] [--out dist] [--bin-dir <dir>] [--dry-run]`: stages the
//! Windows portable tree (release_pack's layout) and writes camcraft-<ver>-win-x64.zip - nothing else
//! (no bump, no commit, no build). The one implementation behind `cargo xtask release`'s zip phase and
//! release.yml's Windows job; under GitHub Actions it reports the version, stage dir and zip to
//! $GITHUB_OUTPUT and CAMCRAFT_VERSION to $GITHUB_ENV for the installer and publish steps.
//! Provenance: our own release law (AGENTS.md "Commits and Releases"), #752 (one zip layout, not two).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::decisions::valid_version;
use crate::release_pack::{self as pack, Os};

/// Parsed `package` options. Relative paths are taken from the workspace root.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PackageArgs {
    /// None: read from `<bin-dir>/post.exe --version` (the binary we ship stamps its own version).
    pub version: Option<String>,
    pub out: Option<PathBuf>,
    pub bin_dir: Option<PathBuf>,
    pub dry: bool,
}

pub fn parse(args: &[String]) -> Result<PackageArgs, String> {
    let mut p = PackageArgs::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{a} needs a value"));
        match a.as_str() {
            "--version" => p.version = Some(value()?),
            "--out" => p.out = Some(PathBuf::from(value()?)),
            "--bin-dir" => p.bin_dir = Some(PathBuf::from(value()?)),
            "--dry-run" => p.dry = true,
            s => return Err(format!("unknown package option '{s}'")),
        }
    }
    Ok(p)
}

/// What a package run made (or, dry, would make).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packed {
    pub version: String,
    pub stage: PathBuf,
    pub zip: PathBuf,
    pub files: usize,
}

impl Packed {
    /// The lines for $GITHUB_OUTPUT (the job's `version` output and the installer's stage) and
    /// $GITHUB_ENV (CAMCRAFT_VERSION, which the installer step reads).
    pub fn github_output(&self) -> Vec<String> {
        vec![
            format!("version={}", self.version),
            format!("stage={}", self.stage.display()),
            format!("zip={}", self.zip.display()),
        ]
    }
    pub fn github_env(&self) -> Vec<String> {
        vec![format!("CAMCRAFT_VERSION={}", self.version)]
    }
}

fn under(root: &Path, p: &Path) -> PathBuf {
    if p.is_absolute() { p.to_path_buf() } else { root.join(p) }
}

/// The version `post --version` reports ("post X.Y.Z ..."): its second word.
pub fn version_from_post(text: &str) -> Option<String> {
    text.split_whitespace().nth(1).map(str::to_string).filter(|v| valid_version(v))
}

fn read_version(post: &Path) -> Result<String, String> {
    let out = Command::new(post)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{}: {e} (build first, or pass --version)", post.display()))?;
    if !out.status.success() {
        return Err(format!("{} --version failed", post.display()));
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    version_from_post(&text).ok_or_else(|| format!("cannot read a version from '{}'", text.trim()))
}

/// The PowerShell that writes the zip: pwsh (7) when on PATH - its Archive module writes '/' entry
/// names, as release.yml's pwsh step always did - else Windows PowerShell 5.1.
fn shell() -> &'static str {
    let found = std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("pwsh.exe").is_file() || d.join("pwsh").is_file()));
    if found { "pwsh" } else { "powershell" }
}

/// The Compress-Archive arguments that zip `stage` (the folder itself, as the old workflow step did)
/// into `zip`.
pub fn zip_args(stage: &Path, zip: &Path) -> Vec<String> {
    let q = |p: &Path| p.display().to_string().replace('\'', "''");
    let cmd = format!("Compress-Archive -Path '{}' -DestinationPath '{}' -Force", q(stage), q(zip));
    vec!["-NoProfile".into(), "-NonInteractive".into(), "-Command".into(), cmd]
}

/// Stages the tree and writes the zip; `say` gets every line worth printing. A real run is Windows
/// only (the zip is the win-x64 package and Compress-Archive is PowerShell's); a dry run lists the
/// plan on any host.
pub fn package(root: &Path, a: &PackageArgs, os: Os, say: &mut dyn FnMut(String)) -> Result<Packed, String> {
    if !a.dry && os != Os::Windows {
        return Err(format!(
            "the portable zip is the Windows package (camcraft-<ver>-win-x64.zip, Compress-Archive): not built on {}; \
             --dry-run shows its layout anywhere, and {}'s own packages come from scripts/package-*.sh",
            os.name(),
            os.name()
        ));
    }
    let bin = a.bin_dir.as_deref().map_or_else(|| pack::bin_dir(root, Os::Windows), |b| under(root, b));
    let out = a.out.as_deref().map_or_else(|| root.join("dist"), |o| under(root, o));
    let version = match &a.version {
        Some(v) => v.clone(),
        None if a.dry => return Err("a dry run needs --version (it reads no binary)".into()),
        None => read_version(&bin.join("post.exe"))?,
    };
    if !valid_version(&version) {
        return Err(format!("--version must be X.Y.Z, got '{version}'"));
    }
    let name = pack::stage_name(&version);
    let stage = out.join(&name);
    let zip = out.join(format!("{name}.zip"));
    let list = pack::stage_list(root, &bin, Os::Windows)?;
    let packed = Packed { version, stage: stage.clone(), zip: zip.clone(), files: list.len() };
    let args = zip_args(&stage, &zip);
    if a.dry {
        say(format!("[dry-run] stage {} ({} files):", stage.display(), list.len()));
        for (_, rel) in &list {
            say(format!("[dry-run]   {}", rel.to_string_lossy().replace('\\', "/")));
        }
        say(format!("[dry-run] {} {}", shell(), args.join(" ")));
        return Ok(packed);
    }
    let n = pack::stage(&list, &stage)?;
    say(format!("staged {n} files in {}", stage.display()));
    if zip.exists() {
        std::fs::remove_file(&zip).map_err(|e| format!("remove {}: {e}", zip.display()))?;
    }
    let sh = shell();
    say(format!("> {sh} {}", args.join(" ")));
    let st = Command::new(sh)
        .args(&args)
        .current_dir(root)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("{sh}: {e}"))?;
    if !st.success() || !zip.is_file() {
        return Err(format!("Compress-Archive did not write {}", zip.display()));
    }
    say(format!("zip: {}", zip.display()));
    Ok(packed)
}

/// Appends `lines` to the file a GitHub Actions variable ($GITHUB_OUTPUT, $GITHUB_ENV) names.
pub fn append_lines(file: &Path, lines: &[String]) -> Result<(), String> {
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .map_err(|e| format!("{}: {e}", file.display()))?;
    for l in lines {
        writeln!(f, "{l}").map_err(|e| format!("{}: {e}", file.display()))?;
    }
    Ok(())
}

/// `cargo xtask package`: 0 done, 1 refused or failed (named on stderr).
pub fn run(root: &Path, a: &PackageArgs) -> i32 {
    let mut say = |l: String| println!("{l}");
    let packed = match package(root, a, Os::host(), &mut say) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("PACKAGE FAILED: {e}");
            return 1;
        }
    };
    println!("version {}  stage {}  zip {}", packed.version, packed.stage.display(), packed.zip.display());
    if a.dry {
        return 0;
    }
    for (var, lines) in [("GITHUB_OUTPUT", packed.github_output()), ("GITHUB_ENV", packed.github_env())] {
        if let Some(file) = std::env::var_os(var).filter(|v| !v.is_empty()) {
            if let Err(e) = append_lines(Path::new(&file), &lines) {
                eprintln!("PACKAGE FAILED: ${var}: {e}");
                return 1;
            }
            println!("${var}: {}", lines.join(", "));
        }
    }
    0
}
