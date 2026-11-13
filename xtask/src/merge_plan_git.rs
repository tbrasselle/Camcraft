//! `cargo xtask merge-plan`: the git and gh side of the merge planner (#1221). Lists the open PRs, simulates their
//! merges with `git merge-tree --write-tree` and `git commit-tree` (nothing is checked out, built or pushed), and
//! prints the wave plan (merge_plan.rs) or the delta class of one PR since a reviewed head (merge_delta.rs).
//! Provenance: port of the maintainer's merge-ready planner (plan_merge_order.py), #1221.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::merge_delta::{self, Commit};
use crate::merge_plan::{self, Facts, Mergeable, Pr};

/// GitHub computes `mergeable` lazily: after main moves it answers UNKNOWN for a few seconds (reference planner: 6 x 5 s).
const UNKNOWN_RETRIES: usize = 6;
const UNKNOWN_WAIT: Duration = Duration::from_secs(5);

const USAGE: &str = "usage: cargo xtask merge-plan <PR>... | --label <name> [--json] | --delta <PR> --reviewed <sha>
      [--repo owner/name] [--remote origin] [--base main]";

/// Commit identity for the simulated merges (they are never pushed).
const IDENTITY: [(&str, &str); 4] = [
    ("GIT_AUTHOR_NAME", "merge-plan"),
    ("GIT_AUTHOR_EMAIL", "merge-plan@localhost"),
    ("GIT_COMMITTER_NAME", "merge-plan"),
    ("GIT_COMMITTER_EMAIL", "merge-plan@localhost"),
];

/// The gh list fields: number, head branch, base branch, labels, paths (unit separator), mergeable, title.
const JQ: &str = ".[] | [(.number|tostring), .headRefName, .baseRefName, (.labels|map(.name)|join(\",\")), \
     (.files|map(.path)|join(\"\\u001f\")), .mergeable, (.title|gsub(\"[\\t\\r\\n]\";\" \"))] | join(\"\\t\")";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub prs: Vec<u32>,
    pub label: Option<String>,
    pub delta: Option<u32>,
    pub reviewed: Option<String>,
    pub repo: Option<String>,
    pub remote: String,
    pub base: String,
    /// `--json`: print the plan as JSON (merge_plan_out.rs).
    pub json: bool,
}

fn take(it: &mut std::slice::Iter<'_, String>, flag: &str) -> Result<String, String> {
    it.next().cloned().ok_or_else(|| format!("{flag} needs a value\n{USAGE}"))
}

pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args { remote: "origin".into(), base: "main".into(), ..Args::default() };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--json" => a.json = true,
            "--label" => a.label = Some(take(&mut it, arg)?),
            "--repo" => a.repo = Some(take(&mut it, arg)?),
            "--remote" => a.remote = take(&mut it, arg)?,
            "--base" => a.base = take(&mut it, arg)?,
            "--reviewed" => a.reviewed = Some(take(&mut it, arg)?),
            "--delta" => {
                let v = take(&mut it, arg)?;
                a.delta = Some(v.parse().map_err(|_| format!("--delta needs a PR number, got '{v}'"))?);
            }
            s if s.starts_with('-') => return Err(format!("unknown merge-plan option '{s}'\n{USAGE}")),
            s => a.prs.push(s.parse().map_err(|_| format!("'{s}' is not a PR number\n{USAGE}"))?),
        }
    }
    if a.delta.is_some() && a.reviewed.is_none() {
        return Err(format!("--delta needs --reviewed <sha of the reviewed head>\n{USAGE}"));
    }
    if a.delta.is_none() && a.prs.is_empty() && a.label.is_none() {
        return Err(format!("give PR numbers, --label, or --delta\n{USAGE}"));
    }
    Ok(a)
}

/// `cargo xtask merge-plan ...`: prints the plan (or the delta class); the process exit code.
pub fn run(root: &Path, args: &[String]) -> Result<i32, String> {
    let a = parse(args)?;
    if let (Some(pr), Some(reviewed)) = (a.delta, a.reviewed.as_deref()) {
        let result = delta(root, &a, pr, reviewed);
        cleanup_refs(root, &[pr]);
        print!("{}", result?);
        return Ok(0);
    }
    let listed = load(root, &a)?;
    let numbers: Vec<u32> = listed.iter().map(|p| p.number).collect();
    let result = plan_for(root, &a, listed);
    cleanup_refs(root, &numbers);
    let plan = result?;
    print!("{}", if a.json { merge_plan::to_json(&plan) } else { merge_plan::render(&plan) });
    Ok(0)
}

/// Deletes the `refs/merge-plan/<N>` refs this run fetched (they are scratch refs; the simulated commits stay as loose
/// objects). A failure is printed, never silent, and does not hide the plan's own result.
fn cleanup_refs(root: &Path, numbers: &[u32]) {
    for n in numbers {
        if let Err(e) = git(root, &["update-ref", "-d", &format!("refs/merge-plan/{n}")]) {
            eprintln!("warning: could not delete refs/merge-plan/{n}: {e}");
        }
    }
}

/// `gh` stdout, or the error gh printed.
fn gh(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("gh")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|e| format!("gh {}: {e}", args.join(" ")))?;
    if !out.status.success() {
        return Err(format!("gh {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `git -C root <args>`: the exit code and stdout, whatever the code (merge-tree exits 1 on a conflict).
fn git_status(root: &Path, args: &[&str]) -> Result<(i32, String), String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .envs(IDENTITY)
        .output()
        .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    Ok((out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned()))
}

/// `git -C root <args>`: trimmed stdout; an error when git exits non-zero.
fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let (code, out) = git_status(root, args)?;
    if code != 0 {
        return Err(format!("git {} failed (exit {code})", args.join(" ")));
    }
    Ok(out.trim().to_string())
}

fn is_ancestor(root: &Path, a: &str, b: &str) -> Result<bool, String> {
    Ok(git_status(root, &["merge-base", "--is-ancestor", a, b])?.0 == 0)
}

/// One open PR as gh lists it, plus the head sha once fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Listed {
    number: u32,
    title: String,
    head_ref: String,
    base_ref: String,
    labels: Vec<String>,
    paths: Vec<String>,
    mergeable: Mergeable,
    head: String,
}

fn parse_row(line: &str) -> Result<Listed, String> {
    let f: Vec<&str> = line.splitn(7, '\t').collect();
    let [number, head_ref, base_ref, labels, paths, mergeable, title] = f.as_slice() else {
        return Err(format!("unexpected gh row: {line}"));
    };
    Ok(Listed {
        number: number.parse().map_err(|_| format!("bad PR number in gh row: {line}"))?,
        title: title.to_string(),
        head_ref: head_ref.to_string(),
        base_ref: base_ref.to_string(),
        labels: labels.split(',').filter(|l| !l.is_empty()).map(str::to_string).collect(),
        paths: paths.split('\u{1f}').filter(|p| !p.is_empty()).map(str::to_string).collect(),
        mergeable: Mergeable::parse(mergeable),
        head: String::new(),
    })
}

/// Every open PR, as gh lists it now.
fn fetch_listed(root: &Path, a: &Args) -> Result<Vec<Listed>, String> {
    let mut cmd = vec![
        "pr", "list", "--state", "open", "--limit", "300", "--json",
        "number,title,headRefName,baseRefName,files,labels,mergeable", "--jq", JQ,
    ];
    if let Some(r) = a.repo.as_deref() {
        cmd.extend(["--repo", r]);
    }
    gh(root, &cmd)?.lines().filter(|l| !l.is_empty()).map(parse_row).collect()
}

/// The open PRs the plan covers: by label, or the numbers given (each must be open). A PR whose `mergeable` is
/// still UNKNOWN is read again after a short wait, a few times; the plan warns about any that stay unknown.
fn load(root: &Path, a: &Args) -> Result<Vec<Listed>, String> {
    let mut chosen = select(fetch_listed(root, a)?, a)?;
    for _ in 0..UNKNOWN_RETRIES {
        if !chosen.iter().any(|p| p.mergeable == Mergeable::Unknown) {
            break;
        }
        thread::sleep(UNKNOWN_WAIT);
        chosen = select(fetch_listed(root, a)?, a)?;
    }
    Ok(chosen)
}

fn select(all: Vec<Listed>, a: &Args) -> Result<Vec<Listed>, String> {
    let mut chosen: Vec<Listed> = if let Some(label) = &a.label {
        all.into_iter().filter(|p| p.labels.contains(label)).collect()
    } else {
        let want: BTreeSet<u32> = a.prs.iter().copied().collect();
        let found: BTreeSet<u32> = all.iter().map(|p| p.number).filter(|n| want.contains(n)).collect();
        let missing: Vec<u32> = want.difference(&found).copied().collect();
        if !missing.is_empty() {
            return Err(format!("not open PRs: {missing:?}"));
        }
        all.into_iter().filter(|p| want.contains(&p.number)).collect()
    };
    if chosen.is_empty() {
        return Err("no matching open PRs".into());
    }
    chosen.sort_by_key(|p| p.number);
    Ok(chosen)
}

/// Simulated merges: the commit of base plus a sequence of PRs, built once per prefix.
struct Sim<'a> {
    root: &'a Path,
    base_sha: String,
    heads: BTreeMap<u32, String>,
    commits: BTreeMap<Vec<u32>, String>,
}

impl Sim<'_> {
    fn head(&self, pr: u32) -> Result<String, String> {
        self.heads.get(&pr).cloned().ok_or_else(|| format!("no fetched head for PR #{pr}"))
    }

    /// The commit of base with `seq` merged in order. Only sequences the planner has seen clean are asked for.
    fn commit_of(&mut self, seq: &[u32]) -> Result<String, String> {
        let Some((last, rest)) = seq.split_last() else { return Ok(self.base_sha.clone()) };
        if let Some(c) = self.commits.get(seq) {
            return Ok(c.clone());
        }
        let cur = self.commit_of(rest)?;
        let head = self.head(*last)?;
        let (code, out) = git_status(self.root, &["merge-tree", "--write-tree", "--name-only", "--no-messages", &cur, &head])?;
        if code != 0 {
            return Err(format!("PR #{last} does not merge cleanly in this sequence; the planner should not have asked"));
        }
        let tree = out.lines().next().unwrap_or("").trim().to_string();
        if tree.is_empty() {
            return Err(format!("git merge-tree gave no tree for PR #{last}"));
        }
        let msg = format!("simulated merge of PR {last}");
        let c = git(self.root, &["commit-tree", &tree, "-p", &cur, "-p", &head, "-m", &msg])?;
        self.commits.insert(seq.to_vec(), c.clone());
        Ok(c)
    }

    /// `None` when `pr` merges cleanly on `onto`, else the conflicted paths.
    fn trial(&mut self, onto: &[u32], pr: u32) -> Result<Option<Vec<String>>, String> {
        let cur = self.commit_of(onto)?;
        let head = self.head(pr)?;
        let (code, out) = git_status(self.root, &["merge-tree", "--write-tree", "--name-only", "--no-messages", &cur, &head])?;
        match code {
            0 => Ok(None),
            1 => Ok(Some(out.lines().skip(1).filter(|l| !l.is_empty()).map(str::to_string).collect())),
            c => Err(format!("git merge-tree failed (exit {c}) for PR #{pr}")),
        }
    }
}

fn count(root: &Path, range: &str) -> Result<usize, String> {
    git(root, &["rev-list", "--count", range])?.parse().map_err(|_| format!("bad commit count for {range}"))
}

/// Fetches the PRs and their base, works out the stack and shared-history facts, then runs the planner.
fn plan_for(root: &Path, a: &Args, mut listed: Vec<Listed>) -> Result<merge_plan::Plan, String> {
    let (remote, base) = (a.remote.as_str(), a.base.as_str());
    let refspecs: Vec<String> = listed.iter().map(|p| format!("+refs/pull/{0}/head:refs/merge-plan/{0}", p.number)).collect();
    let mut fetch: Vec<&str> = vec!["fetch", "--quiet", remote, base];
    fetch.extend(refspecs.iter().map(String::as_str));
    git(root, &fetch)?;
    let base_sha = git(root, &["rev-parse", &format!("refs/remotes/{remote}/{base}")])?;
    for p in &mut listed {
        p.head = git(root, &["rev-parse", &format!("refs/merge-plan/{}", p.number)])?;
    }

    // a must merge before b: b is stacked on a's head branch, or a's head is in b's history.
    let mut before: merge_plan::Before = BTreeMap::new();
    for pa in &listed {
        for pb in &listed {
            if pa.number == pb.number {
                continue;
            }
            let linked = pb.base_ref == pa.head_ref || (pa.head != pb.head && is_ancestor(root, &pa.head, &pb.head)?);
            if linked {
                before.entry(pb.number).or_default().insert(pa.number);
            }
        }
    }

    // Pairs with unmerged shared history (a stack whose parent moved on): each side's own commits above the base.
    let mut shared = BTreeMap::new();
    for (i, pa) in listed.iter().enumerate() {
        for pb in &listed[i + 1..] {
            let (code, mb) = git_status(root, &["merge-base", &pa.head, &pb.head])?;
            let mb = mb.trim().to_string();
            if code != 0 || mb.is_empty() || is_ancestor(root, &mb, &base_sha)? {
                continue;
            }
            let own_a = count(root, &format!("{mb}..{}", pa.head))?;
            let own_b = count(root, &format!("{mb}..{}", pb.head))?;
            shared.insert((pa.number, pb.number), (own_a, own_b));
        }
    }

    // Merge bases matter only where GitHub reports a conflict: criss-cross history gives several.
    let mut prs: Vec<Pr> = Vec::new();
    for p in &listed {
        let merge_bases = match p.mergeable {
            Mergeable::Conflicting => match git_status(root, &["merge-base", "--all", &base_sha, &p.head])? {
                (0, out) => out.lines().filter(|l| !l.trim().is_empty()).count(),
                _ => 0,
            },
            Mergeable::Clean | Mergeable::Unknown => 0,
        };
        prs.push(Pr {
            number: p.number,
            title: p.title.clone(),
            head: p.head.clone(),
            paths: p.paths.clone(),
            mergeable: p.mergeable,
            merge_bases,
        });
    }
    let heads: BTreeMap<u32, String> = listed.iter().map(|p| (p.number, p.head.clone())).collect();
    let facts = Facts { base_name: base.to_string(), base_sha: base_sha.clone(), prs, before, shared };
    let mut sim = Sim { root, base_sha, heads, commits: BTreeMap::new() };
    merge_plan::plan(&facts, &mut |onto: &[u32], pr: u32| sim.trial(onto, pr))
}

/// The delta class of PR `pr` since `reviewed`: fetched head, the commits after the review, each merge checked.
fn delta(root: &Path, a: &Args, pr: u32, reviewed_arg: &str) -> Result<String, String> {
    let (remote, base) = (a.remote.as_str(), a.base.as_str());
    let refspec = format!("+refs/pull/{pr}/head:refs/merge-plan/{pr}");
    git(root, &["fetch", "--quiet", remote, base, &refspec])?;
    let base_ref = format!("refs/remotes/{remote}/{base}");
    let head = git(root, &["rev-parse", &format!("refs/merge-plan/{pr}")])?;
    let reviewed = git(root, &["rev-parse", reviewed_arg])?;
    let ancestor = is_ancestor(root, &reviewed, &head)?;
    let mut commits: Vec<Commit> = Vec::new();
    if head != reviewed && ancestor {
        let range = format!("{reviewed}..{head}");
        let shas = git(root, &["rev-list", "--reverse", "--first-parent", &range])?;
        for sha in shas.lines().filter(|l| !l.is_empty()) {
            let parents: Vec<String> = git(root, &["rev-list", "--parents", "-n", "1", sha])?
                .split_whitespace()
                .skip(1)
                .map(str::to_string)
                .collect();
            let subject = git(root, &["show", "-s", "--format=%s", sha])?;
            // Only a two-parent merge can be a merge of main: git skips the remerge-diff of an octopus.
            let second_is_base = match parents.as_slice() {
                [_, second] => is_ancestor(root, second, &base_ref)?,
                _ => false,
            };
            let remerge_files = if second_is_base {
                git(root, &["show", "--remerge-diff", "--format=", "--name-only", sha])?
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(str::to_string)
                    .collect()
            } else {
                Vec::new()
            };
            commits.push(Commit { sha: sha.to_string(), subject, parents, second_is_base, remerge_files });
        }
    }
    let d = merge_delta::classify(&head, &reviewed, ancestor, base, &commits);
    Ok(merge_delta::render(&d))
}
