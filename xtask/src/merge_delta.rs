//! What changed since a reviewed head (AGENTS.md "A clean merge of main keeps its review", #1221): classifies the
//! commits after the reviewed head, so a merge of main whose remerge-diff is empty keeps its review and needs no
//! delta review. Pure: git supplies the commits (merge_plan_git.rs). Provenance: port of the maintainer's
//! merge-ready planner delta mode (plan_merge_order.py --delta, #1221), the same verdicts and notes.

use std::fmt::Write as _;

use crate::merge_plan::{mechanical, short};

/// One commit after the reviewed head, as git describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub sha: String,
    pub subject: String,
    /// The parent shas, first parent first.
    pub parents: Vec<String>,
    /// For a merge: its second parent is an ancestor of the base, so the merge brings main in.
    pub second_is_base: bool,
    /// The files `git show --remerge-diff` changes for a merge of the base (empty: no hand edits).
    pub remerge_files: Vec<String>,
}

/// The class of the change since the review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The head is the reviewed head.
    Unchanged,
    /// The reviewed head is no longer in the branch: the history was rewritten.
    Rewritten,
    /// Only merges of the base with no hand edits: the review stands.
    Same,
    /// Hand edits only in rule-settled files: read them, no agent.
    Mechanical,
    /// A conflict was resolved by hand: a delta review of the remerge-diff.
    Delta,
    /// New commits (or a merge of another branch): a review of those commits.
    NewCode,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Unchanged | Verdict::Same => "SAME",
            Verdict::Mechanical => "MECHANICAL",
            Verdict::Delta => "DELTA",
            Verdict::Rewritten | Verdict::NewCode => "NEW CODE",
        }
    }

    pub fn meaning(self) -> &'static str {
        match self {
            Verdict::Unchanged => "the head is the reviewed head",
            Verdict::Rewritten => "the reviewed head is not in the branch any more (history rewritten) - full review",
            Verdict::Same => {
                "only merges of the base with no hand edits: the tree is what merging the reviewed head would give - the review stands, no agent needed"
            }
            Verdict::Mechanical => {
                "hand edits only in rule-settled files: read `git show --remerge-diff <merge>` yourself, no agent needed"
            }
            Verdict::Delta => {
                "a conflict was resolved by hand: a read-only delta review of `git show --remerge-diff <merge>` (rebuild only if logic was merged)"
            }
            Verdict::NewCode => "new commits since the review: a review of `git diff <reviewed>..<head>` for those commits",
        }
    }
}

/// The verdict and one note per commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delta {
    pub verdict: Verdict,
    pub notes: Vec<String>,
}

fn subject(s: &str) -> String {
    s.chars().take(70).collect()
}

/// Classifies the commits between the reviewed head and `head`. `commits` is in order, first parent only.
pub fn classify(head: &str, reviewed: &str, reviewed_is_ancestor: bool, base_name: &str, commits: &[Commit]) -> Delta {
    if head == reviewed {
        return Delta { verdict: Verdict::Unchanged, notes: Vec::new() };
    }
    if !reviewed_is_ancestor {
        return Delta { verdict: Verdict::Rewritten, notes: Vec::new() };
    }
    let mut verdict = Verdict::Same;
    let mut notes = Vec::new();
    for c in commits {
        let sha = short(&c.sha);
        // git skips the remerge-diff of an octopus merge, so only the second parent is no evidence of a merge of main.
        if c.parents.len() > 2 {
            verdict = Verdict::NewCode;
            notes.push(format!("{sha} octopus merge ({} parents): {}", c.parents.len(), subject(&c.subject)));
            continue;
        }
        let Some(second) = c.parents.get(1) else {
            verdict = Verdict::NewCode;
            notes.push(format!("{sha} commit: {}", subject(&c.subject)));
            continue;
        };
        if !c.second_is_base {
            verdict = Verdict::NewCode;
            notes.push(format!(
                "{sha} merges a branch that is not {base_name} ({}): {}",
                short(second),
                subject(&c.subject)
            ));
            continue;
        }
        if c.remerge_files.is_empty() {
            notes.push(format!("{sha} merge of {base_name}, no hand edits"));
        } else if c.remerge_files.iter().all(|f| mechanical(f)) {
            notes.push(format!("{sha} merge of {base_name}, hand edits only in: {}", c.remerge_files.join(", ")));
            if verdict == Verdict::Same {
                verdict = Verdict::Mechanical;
            }
        } else {
            notes.push(format!("{sha} merge of {base_name}, HAND-RESOLVED: {}", c.remerge_files.join(", ")));
            if verdict != Verdict::NewCode {
                verdict = Verdict::Delta;
            }
        }
    }
    Delta { verdict, notes }
}

/// The verdict as the text the skill reads: a label, its meaning, then the notes.
pub fn render(d: &Delta) -> String {
    let mut s = format!("{}: {}\n", d.verdict.label(), d.verdict.meaning());
    for n in &d.notes {
        let _ = writeln!(s, "  {n}");
    }
    s
}
