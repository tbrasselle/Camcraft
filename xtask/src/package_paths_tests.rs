//! Pins that every repo file the packaging scripts and release.yml install exists in the tree (#1428:
//! v0.19.0's linux and macos jobs failed on a deleted `assets/ui/cube.png`). Runs on every OS: it reads
//! the scripts as text, never runs them.
//! Provenance: our own release law (AGENTS.md "Commits and Releases"), #1428.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().map(Path::to_path_buf).unwrap_or_default()
}

/// Literal `assets/...` paths in `text` (a path ends at whitespace, a quote, `$` or a shell metachar).
/// Paths holding a variable (`assets/$asset`) are skipped; their directories are checked separately.
fn asset_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (at, _) in text.match_indices("assets/") {
        if at > 0 && text[..at].chars().next_back().is_some_and(|c| c.is_alphanumeric() || "_-/".contains(c)) {
            continue;
        }
        let tail = &text[at..];
        let end = tail.find(|c: char| !(c.is_alphanumeric() || "/._-".contains(c))).unwrap_or(tail.len());
        let path = tail[..end].trim_end_matches(['.', '/']);
        let followed_by_var = tail[end..].starts_with('$');
        if !followed_by_var && path.len() > "assets/".len() && !out.iter().any(|p| p == path) {
            out.push(path.to_string());
        }
    }
    out
}

#[test]
fn every_asset_a_packaging_script_names_exists() {
    let root = root();
    let mut missing = Vec::new();
    let mut seen = 0;
    let scripts = std::fs::read_dir(root.join("scripts")).map(|d| d.flatten().collect::<Vec<_>>()).unwrap_or_default();
    let mut files: Vec<PathBuf> = scripts.into_iter().map(|e| e.path()).filter(|p| {
        p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("package-") && n.ends_with(".sh"))
    }).collect();
    files.push(root.join(".github/workflows/release.yml"));
    for f in &files {
        let Ok(text) = std::fs::read_to_string(f) else { continue };
        for p in asset_paths(&text) {
            seen += 1;
            if !root.join(&p).exists() {
                missing.push(format!("{} names {p}", f.strip_prefix(&root).unwrap_or(f).display()));
            }
        }
    }
    assert!(seen > 0, "no asset path found in the packaging scripts: the scan itself is broken");
    assert!(missing.is_empty(), "packaging names files missing from the tree: {missing:?}");
}

#[test]
fn the_asset_directories_the_scripts_loop_over_exist() {
    let root = root();
    for script in ["appimage", "freebsd", "linux", "macos"] {
        let Ok(text) = std::fs::read_to_string(root.join(format!("scripts/package-{script}.sh"))) else { continue };
        let Some(line) = text.lines().find(|l| l.trim_start().starts_with("for asset in ")) else { continue };
        for dir in line.trim_start().trim_start_matches("for asset in ").trim_end_matches("; do").split_whitespace() {
            assert!(root.join("assets").join(dir).is_dir(), "package-{script}.sh copies assets/{dir}, which is missing");
        }
    }
}

#[test]
fn the_scan_sees_a_literal_path_and_skips_variable_ones() {
    let got = asset_paths("convert assets/ui/cube.png -resize x; cp -a \"assets/$asset\" y; cp \"$x/assets/z\"");
    assert_eq!(got, vec!["assets/ui/cube.png".to_string()]);
}
