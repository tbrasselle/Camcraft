//! What a release cut builds and ships per OS: the product packages, the binaries' dir, the portable
//! zip's staged tree (the same layout as .github/workflows/release.yml's Windows job) and the
//! packaging script each OS hands off to.
//! Provenance: our own release law (AGENTS.md "Commits and Releases"), ported from scripts/release.ps1
//! phases 5-8 and release.yml's staging (#749).

use std::path::{Path, PathBuf};

/// The host a cut runs on: decides the exe suffix, signing, the zip and the packager.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    Linux,
    Macos,
    FreeBsd,
    Other,
}

impl Os {
    pub fn host() -> Os {
        match std::env::consts::OS {
            "windows" => Os::Windows,
            "linux" => Os::Linux,
            "macos" => Os::Macos,
            "freebsd" => Os::FreeBsd,
            _ => Os::Other,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Os::Windows => "windows",
            Os::Linux => "linux",
            Os::Macos => "macos",
            Os::FreeBsd => "freebsd",
            Os::Other => std::env::consts::OS,
        }
    }
    pub fn exe(self) -> &'static str {
        if self == Os::Windows { ".exe" } else { "" }
    }
    /// The script that packages this OS's release, as release.yml runs it (AppImage, snap and flatpak
    /// need CI-only tooling and stay in release.yml). Windows packs its zip here, in Rust.
    pub fn packager(self) -> Option<&'static str> {
        match self {
            Os::Linux => Some("scripts/package-linux.sh"),
            Os::Macos => Some("scripts/package-macos.sh"),
            Os::FreeBsd => Some("scripts/package-freebsd.sh"),
            Os::Windows | Os::Other => None,
        }
    }
}

/// The shipped executables (names without the OS suffix).
pub const EXES: &[&str] = &["camcraft-ui", "camcraft-cli", "post"];
/// The packages the release build compiles - the product only, as release.yml does. Never `xtask`:
/// on Windows the running xtask.exe is locked, and rebuilding it mid-cut fails the build.
pub const PRODUCT: &[&str] = &["camcraft-ui", "camcraft-app", "camcraft-post"];
/// Asset dirs copied beside the executables, whole.
pub const ASSET_DIRS: &[&str] = &["assets/posts", "assets/machines", "assets/ui", "assets/scripts", "assets/themes"];
/// Top-level files copied beside the executables (release.yml ships NOTICE too).
pub const DOCS: &[&str] = &["RELEASE_NOTES.md", "TESTING.md", "LICENSE", "NOTICE"];

/// The macOS triple of this host's architecture; the universal build makes both.
pub fn mac_triple() -> &'static str {
    if std::env::consts::ARCH == "aarch64" { "aarch64-apple-darwin" } else { "x86_64-apple-darwin" }
}

/// cargo's target dir: CARGO_TARGET_DIR (relative to the workspace root) when set, else `target`.
pub fn target_dir(root: &Path) -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR").filter(|v| !v.is_empty()) {
        Some(v) => root.join(v),
        None => root.join("target"),
    }
}

/// Where the release build leaves this host's executables.
pub fn bin_dir(root: &Path, os: Os) -> PathBuf {
    let t = target_dir(root);
    if os == Os::Macos { t.join(mac_triple()).join("release") } else { t.join("release") }
}

/// The cargo argument lists of the release build: the product packages, one build per target (macOS
/// builds both architectures for the universal app, as release.yml does).
pub fn build_steps(os: Os) -> Vec<Vec<String>> {
    let base = |target: Option<&str>| {
        let mut a: Vec<String> = ["build", "-j", "6", "--release"].iter().map(|s| (*s).to_string()).collect();
        if let Some(t) = target {
            a.push("--target".into());
            a.push(t.into());
        }
        for p in PRODUCT {
            a.push("-p".into());
            a.push((*p).into());
        }
        a
    };
    if os == Os::Macos {
        vec![base(Some("aarch64-apple-darwin")), base(Some("x86_64-apple-darwin"))]
    } else {
        vec![base(None)]
    }
}

/// The Windows zip's stage dir name; the zip and the installer are named after it.
pub fn stage_name(version: &str) -> String {
    format!("camcraft-{version}-win-x64")
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e} (nothing to stage)", dir.display()))?;
    let mut entries: Vec<PathBuf> = rd.filter_map(Result::ok).map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            files_under(&p, out)?;
        } else {
            out.push(p);
        }
    }
    Ok(())
}

/// Every file of the staged tree: (source, path inside the stage). The executables come from `bin`
/// (they need not exist yet: a dry run lists them all the same); the assets and docs from `root`.
pub fn stage_list(root: &Path, bin: &Path, os: Os) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let mut list = Vec::new();
    for e in EXES {
        let name = format!("{e}{}", os.exe());
        list.push((bin.join(&name), PathBuf::from(name)));
    }
    for d in ASSET_DIRS {
        let mut files = Vec::new();
        files_under(&root.join(d), &mut files)?;
        for f in files {
            let rel = f.strip_prefix(root).map_err(|e| format!("{}: {e}", f.display()))?.to_path_buf();
            list.push((f, rel));
        }
    }
    for d in DOCS {
        let p = root.join(d);
        if !p.is_file() {
            return Err(format!("{d} missing: nothing to stage"));
        }
        list.push((p, PathBuf::from(d)));
    }
    Ok(list)
}

/// Builds the stage dir afresh from the list; the number of files copied.
pub fn stage(list: &[(PathBuf, PathBuf)], dir: &Path) -> Result<usize, String> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| format!("remove {}: {e}", dir.display()))?;
    }
    for (src, rel) in list {
        let dst = dir.join(rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        std::fs::copy(src, &dst).map_err(|e| format!("copy {} -> {}: {e}", src.display(), dst.display()))?;
    }
    Ok(list.len())
}
