//! The text edits and checks of a release cut: the RELEASE_NOTES.md section check, the workspace
//! version bump in Cargo.toml and Cargo.lock, the Cargo.lock diff guard and the preflight's dirty-tree
//! filter. Pure functions on text, so the unit tests pin them on fixtures.
//! Provenance: our own release law (AGENTS.md "Commits and Releases", #272), ported from
//! scripts/release.ps1 phases 1 and 3 (#749); Cargo.lock's format per the Cargo book ("Cargo.lock").

use crate::scan::re;

/// Files a cut may find changed before it starts: it commits them itself.
pub const ALLOWED_DIRTY: &[&str] = &["RELEASE_NOTES.md", "TESTING.md", "Cargo.toml", "Cargo.lock", "RECOMMENDED.md"];

/// The paths `git status --porcelain` reports that a cut does not commit itself.
pub fn unexpected_changes(porcelain: &str) -> Vec<String> {
    porcelain
        .lines()
        .filter(|l| l.len() > 3)
        .map(|l| l.get(3..).unwrap_or_default().trim_matches('"').to_string())
        .filter(|p| !ALLOWED_DIRTY.contains(&p.as_str()))
        .collect()
}

/// RELEASE_NOTES.md keeps every release, newest first; release.yml's publish job takes everything
/// before the first `---` line as this version's notes. The section must be there, be this version's
/// and not be empty (#272). The section's line count on success.
pub fn check_notes(text: &str, version: &str) -> Result<usize, String> {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let end = lines.iter().position(|l| *l == "---").ok_or_else(|| {
        "RELEASE_NOTES.md has no '---' line: put this version's section on top of the history, separated by '---'"
            .to_string()
    })?;
    if end <= 1 {
        return Err(format!("RELEASE_NOTES.md has no notes for {version} before its first '---'"));
    }
    let head = lines.first().map(|h| h.trim_start_matches('\u{feff}')).unwrap_or_default();
    if !head.contains(version) {
        return Err(format!("RELEASE_NOTES.md's first section is '{head}', not {version}"));
    }
    if lines.get(1..end).unwrap_or_default().join("\n").trim().is_empty() {
        return Err(format!("RELEASE_NOTES.md's {version} section is empty"));
    }
    Ok(end - 1)
}

/// `[workspace.package] version = "..."` set to `new`: (old version, new text). Everything else in the
/// manifest is kept byte for byte.
pub fn bump_toml(text: &str, new: &str) -> Result<(String, String), String> {
    let r = re(r#"(?s)(\[workspace\.package\].*?\r?\nversion\s*=\s*")([^"]+)(")"#)?;
    let m = r
        .captures(text)
        .and_then(|c| c.get(2))
        .ok_or_else(|| "no version in [workspace.package]".to_string())?;
    let (head, tail) = (text.get(..m.start()), text.get(m.end()..));
    let (head, tail) = head.zip(tail).ok_or_else(|| "version span off a char boundary".to_string())?;
    Ok((m.as_str().to_string(), format!("{head}{new}{tail}")))
}

/// Every `[[package]]` of Cargo.lock without a `source` line (a workspace path package) at version
/// `old` moved to `new`: (new text, packages changed). Registry and git packages are never touched,
/// whatever their version. `cargo update -w` afterwards finds nothing left to do.
pub fn bump_lock(text: &str, old: &str, new: &str) -> (String, usize) {
    let from = format!("version = \"{old}\"");
    let to = format!("version = \"{new}\"");
    let mut blocks: Vec<Vec<&str>> = vec![Vec::new()];
    for line in text.split_inclusive('\n') {
        if line.trim_end() == "[[package]]" {
            blocks.push(Vec::new());
        }
        if let Some(b) = blocks.last_mut() {
            b.push(line);
        }
    }
    let (mut out, mut n) = (String::with_capacity(text.len()), 0);
    for (i, block) in blocks.iter().enumerate() {
        let local = i > 0 && !block.iter().any(|l| l.starts_with("source = "));
        for line in block {
            if local && line.trim_end() == from {
                out.push_str(&line.replacen(&from, &to, 1));
                n += 1;
            } else {
                out.push_str(line);
            }
        }
    }
    (out, n)
}

/// `git diff Cargo.lock` after the bump may change version lines only: the number changed, or the
/// refusal.
pub fn lock_diff_versions_only(diff: &str) -> Result<usize, String> {
    let changed = diff
        .lines()
        .filter(|l| {
            let mut c = l.chars();
            matches!(c.next(), Some('-' | '+')) && !matches!(c.next(), None | Some('-' | '+'))
        })
        .count();
    let versions = diff.lines().filter(|l| l.starts_with("-version") || l.starts_with("+version")).count();
    if changed == versions {
        Ok(versions)
    } else {
        Err("Cargo.lock changed beyond version lines".into())
    }
}
