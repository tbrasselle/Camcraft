//! The gate's checkout guard (#1201): HEAD and the working tree are read when a gate starts and again before
//! each step and at its end; a run that straddles a pull, switch or reset is VOID, never a verdict. Detection
//! only - the gate takes no lock, so no stale `index.lock` can block git work in the checkout.
//! Provenance: our own build law (AGENTS.md "Three gate tiers", "Start from the latest main"), #1201.

use std::collections::BTreeSet;
use std::path::Path;

use crate::exec;

/// HEAD's commit and the porcelain status of the checkout at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    pub head: String,
    pub status: String,
}

/// Reads HEAD and `git status --porcelain`. `--no-optional-locks` keeps the read from refreshing the index,
/// so it never writes into the checkout while another git command holds it.
pub fn read(root: &Path) -> Result<Reading, String> {
    let head = exec::git(root, &["rev-parse", "HEAD"])?.trim().to_string();
    let status = exec::git(root, &["--no-optional-locks", "status", "--porcelain"])?;
    Ok(Reading { head, status })
}

/// `None` when the checkout still reads the same; otherwise the VOID line that names what moved and where
/// the change was seen (`at`, e.g. "before step 3" or "at the end").
pub fn void_message(start: &Reading, now: &Reading, at: &str) -> Option<String> {
    if start == now {
        return None;
    }
    let head_moved = start.head != now.head;
    let tree = tree_change(&start.status, &now.status);
    let what = match (head_moved, tree) {
        (true, Some(t)) => format!(
            "the checkout moved from {} to {} and its working tree changed ({t})",
            short(&start.head),
            short(&now.head)
        ),
        (true, None) => format!("the checkout moved from {} to {}", short(&start.head), short(&now.head)),
        (false, Some(t)) => format!("the working tree of {} changed ({t})", short(&now.head)),
        // Equal HEAD and equal status text cannot differ; reached only if git output changed shape.
        (false, None) => format!("the checkout at {} reads differently", short(&now.head)),
    };
    Some(format!("VOID: {what} during the gate ({at}) - this run is void, not a verdict on any commit"))
}

/// The entries of the porcelain status that differ between two readings, up to five named, or `None`.
fn tree_change(before: &str, after: &str) -> Option<String> {
    if before == after {
        return None;
    }
    let b: BTreeSet<&str> = before.lines().collect();
    let a: BTreeSet<&str> = after.lines().collect();
    let diff: Vec<&str> = b.symmetric_difference(&a).copied().collect();
    if diff.is_empty() {
        return Some("status lines reordered".into());
    }
    let shown: Vec<&str> = diff.iter().take(5).copied().collect();
    let more = diff.len().saturating_sub(shown.len());
    let tail = if more > 0 { format!(" and {more} more") } else { String::new() };
    let one = diff.len() == 1;
    let (noun, verb) = if one { ("entry", "differs") } else { ("entries", "differ") };
    Some(format!("{} {noun} {verb}: {}{tail}", diff.len(), shown.join("; ")))
}

fn short(sha: &str) -> String {
    sha.chars().take(8).collect()
}
