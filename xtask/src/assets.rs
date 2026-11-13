//! Quick tier: a changed post script (`assets/posts/*.cps`) or machine file (`assets/machines/*.mch`)
//! selects the suites that post through it (#923).
//! Provenance: our own build law (AGENTS.md "Test the change, not everything"), #919 research.
//!
//! A suite posts through an asset when its source names the file (`fanuc.cps`), quotes its stem
//! (`"fanuc"`, as the conformance tables do), or walks the whole asset directory
//! (`assets/posts/{..}`, `read_dir` of `machines`). An asset no suite names is reported by the plan.

use std::path::Path;

use regex::escape;

use crate::plan::ALL_CRATES;
use crate::scan::{self, re};

/// The asset kinds quick watches: (directory under `assets/`, file extension).
const KINDS: [(&str, &str); 2] = [("posts", "cps"), ("machines", "mch")];

/// One changed asset: its repo path, directory kind and file stem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub path: String,
    pub dir: &'static str,
    pub ext: &'static str,
    pub stem: String,
}

/// The post scripts and machine files among the changed files.
pub fn changed_assets(changed: &[String]) -> Vec<Asset> {
    let mut out = Vec::new();
    for f in changed {
        for (dir, ext) in KINDS {
            let Some(name) = f.strip_prefix(&format!("assets/{dir}/")) else { continue };
            let Some(stem) = name.strip_suffix(&format!(".{ext}")) else { continue };
            if !stem.is_empty() && !stem.contains('/') {
                out.push(Asset { path: f.clone(), dir, ext, stem: stem.to_string() });
            }
        }
    }
    out
}

/// Suites (`crate:binary`) whose source posts through `a`, with the reason.
pub fn suites_for(root: &Path, a: &Asset) -> Result<Vec<(String, String)>, String> {
    let file = re(&format!(r"(?i)\b{}\.{}\b", escape(&a.stem), a.ext))?;
    let quoted = re(&format!(r#"(?i)"{}""#, escape(&a.stem)))?;
    let walk = re(&format!(r#"(?i)assets/{}/\{{|read_dir\([^)]*{}"#, a.dir, a.dir))?;
    let mut out = Vec::new();
    for d in ALL_CRATES {
        for b in scan::binaries(root, d) {
            let text = scan::read(&root.join("crates").join(d).join("tests").join(format!("{b}.rs")));
            let why = if file.is_match(&text) {
                format!("names {}.{}", a.stem, a.ext)
            } else if quoted.is_match(&text) {
                format!("quotes \"{}\"", a.stem)
            } else if walk.is_match(&text) {
                format!("walks assets/{}", a.dir)
            } else {
                continue;
            };
            out.push((format!("{d}:{b}"), why));
        }
    }
    Ok(out)
}
