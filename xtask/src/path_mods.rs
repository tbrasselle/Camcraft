//! Quick-gate module resolution for `#[path = "..."]` files (#1021): a
//! source file a parent module pulls in with `#[path = "x.rs"] mod name;`
//! belongs to that parent, not to a top-level module named after the file.
//! `crates/app/src/posting_index_swing.rs` is `app::posting::index_swing`, and
//! the suites name it `app::posting (index_swing)`. Specced from the Rust
//! reference (the `path` attribute on module declarations).

use std::path::Path;

use crate::scan::re;

/// The module a `#[path]`-included source file also is (the plan maps it beside the file's own
/// name, never instead of it), as the quick plan's
/// `(module, submodule)` pair: `rest` is the file relative to the crate dir
/// (`src/posting_index_swing.rs`). `None` when no file in the crate's `src/`
/// includes it by a `#[path]` attribute (then the file's own name stands).
pub fn included_as(root: &Path, krate: &str, rest: &str) -> Result<Option<(String, String)>, String> {
    let rest = rest.replace('\\', "/");
    let Some(dir_end) = rest.rfind('/') else { return Ok(None) };
    let (dir, file) = (&rest[..dir_end], &rest[dir_end + 1..]);
    let attr = re(&format!(
        r#"#\[path\s*=\s*"{}"\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)"#,
        regex::escape(file)
    ))?;
    let src_file = re(r"(?i)^src/([^/]+?)(\.rs)?(/([^/]+?)(\.rs)?(/.*)?)?$")?;
    let crate_dir = root.join("crates").join(krate);
    // `#[path]` on a module declared in a file resolves against that file's
    // directory, so only the files in the same directory can include it.
    let Ok(entries) = std::fs::read_dir(crate_dir.join(dir)) else { return Ok(None) };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".rs") && n != file)
        .collect();
    names.sort();
    for n in names {
        let raw = std::fs::read_to_string(crate_dir.join(dir).join(&n)).unwrap_or_default();
        // A `#[path]` in a comment declares nothing: drop line comments before matching.
        let text: String = raw.lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n");
        let Some(c) = attr.captures(&text) else { continue };
        let includer = format!("{dir}/{n}");
        let Some(s) = src_file.captures(&includer) else { continue };
        let m = s[1].to_string();
        let sub = match s.get(4).map(|x| x.as_str()) {
            Some(x) if x != "mod" && x != "tests" => x.to_string(),
            _ => c[1].to_string(),
        };
        return Ok(Some((m, sub)));
    }
    Ok(None)
}
