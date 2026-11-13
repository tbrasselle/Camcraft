//! The merge order of a set of open PRs (AGENTS.md "Conflicted PRs are re-merged by their author", #1221):
//! the wave of PRs that merge cleanly with no refresh, then the conflicting ones one at a time.
//! Pure: every conflict comes from a `trial` callback (git's merge-tree, merge_plan_git.rs), so the ordering
//! is unit-tested without git. Provenance: port of the maintainer's merge-ready planner
//! (plan_merge_order.py, #1221), the same steps in the same order.

use std::collections::{BTreeMap, BTreeSet};

pub use crate::merge_plan_out::{render, to_json};

/// A conflict confined to these paths (or to PNG snapshots) is settled by rule, without judgement.
const MECHANICAL: &[&str] = &[".claude/PROCESS_VERSION", "CHANGELOG.md", "RECOMMENDED.md", "Cargo.lock"];
/// Basenames that make a PR "hot": a PR touching one is merged after the cool ones within a wave.
const HOT: &[&str] = &["lib.rs", "mod.rs", "state.rs", "Cargo.lock", "Cargo.toml", "AGENTS.md", "schema.rs"];

/// Conflicts of merging PR `pr` onto the PRs `onto` (already merged, in order) on the base: `None` when clean,
/// else the conflicted paths.
pub type Trial<'a> = dyn FnMut(&[u32], u32) -> Result<Option<Vec<String>>, String> + 'a;

/// Which PR must merge before which: `before[b]` holds the PRs that b is stacked on.
pub type Before = BTreeMap<u32, BTreeSet<u32>>;

/// A conflict that the process settles by rule (AGENTS.md "Conflicted PRs": snapshots, changelog, version).
pub fn mechanical(path: &str) -> bool {
    MECHANICAL.contains(&path) || path.ends_with(".png")
}

/// A short sha for display.
pub fn short(sha: &str) -> String {
    sha.chars().take(8).collect()
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// GitHub's own answer for a PR against the base (`mergeable` in gh): it can disagree with git's merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mergeable {
    Clean,
    Conflicting,
    /// GitHub has not computed it yet (common for a few seconds after main moves).
    Unknown,
}

impl Mergeable {
    /// The gh word (`MERGEABLE`, `CONFLICTING`, anything else is unknown).
    pub fn parse(word: &str) -> Mergeable {
        match word {
            "MERGEABLE" => Mergeable::Clean,
            "CONFLICTING" => Mergeable::Conflicting,
            _ => Mergeable::Unknown,
        }
    }
}

/// Where a PR stands against the base alone, from git's merge and GitHub's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseState {
    /// Merges cleanly: it can join wave 1.
    Sim,
    /// git merges it, GitHub reports a conflict (criss-cross history): a plain refresh, never wave 1.
    GithubOnly,
    /// Conflicts with the base now.
    Conflict,
}

/// The decision table of the base check (reference planner: `github_only` when git is clean and GitHub says
/// CONFLICTING). An UNKNOWN answer is trusted as git says, and reported by the caller.
pub fn base_state(mergeable: Mergeable, git_clean: bool) -> BaseState {
    match (git_clean, mergeable) {
        (false, _) => BaseState::Conflict,
        (true, Mergeable::Conflicting) => BaseState::GithubOnly,
        (true, Mergeable::Clean | Mergeable::Unknown) => BaseState::Sim,
    }
}

/// One open PR as the planner sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pr {
    pub number: u32,
    pub title: String,
    /// The PR's head commit.
    pub head: String,
    /// Every path the PR changes.
    pub paths: Vec<String>,
    /// GitHub's `mergeable` against the base.
    pub mergeable: Mergeable,
    /// Merge bases of the PR's head and the base (only read when GitHub says CONFLICTING; 0 otherwise).
    pub merge_bases: usize,
}

impl Pr {
    /// How many of its paths are hot (see HOT).
    pub fn hot(&self) -> usize {
        self.paths.iter().filter(|p| HOT.contains(&basename(p))).count()
    }

    /// Only documentation and process files.
    pub fn docs(&self) -> bool {
        self.paths.iter().all(|f| f.ends_with(".md") || f.starts_with(".claude/") || f.starts_with("docs/"))
    }
}

/// The facts git supplies that the file lists cannot: stack edges and shared unmerged history.
#[derive(Debug, Default)]
pub struct Facts {
    /// Name of the base branch, for messages (`main`).
    pub base_name: String,
    /// The base commit the merges are simulated on.
    pub base_sha: String,
    pub prs: Vec<Pr>,
    /// Stack edges: `before[b]` must merge before b (its base is a's head branch, or a's head is in b's history).
    pub before: Before,
    /// For a pair `(a, b)` with `a < b` whose merge base is not on the base: the commits each side has above it.
    pub shared: BTreeMap<(u32, u32), (usize, usize)>,
}

/// Why a wave-2 PR refreshes and what its refresh is likely to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Merges cleanly once wave 1 is in: a plain merge of main is enough.
    CleanAfterWave1,
    /// Conflicts only in rule-settled files.
    Mechanical,
    /// Conflicts in code: someone reads the hunk.
    ByHand,
    /// GitHub reports a conflict git does not: a plain merge of main settles it.
    PlainRefresh,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::CleanAfterWave1 => "clean after wave 1",
            Kind::Mechanical => "mechanical",
            Kind::ByHand => "by hand",
            Kind::PlainRefresh => "plain refresh",
        }
    }
}

/// One PR of wave 2: refreshed against the main of that moment, checked, merged, then the next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refresh {
    pub number: u32,
    pub why: String,
    pub files: Vec<String>,
    pub kind: Kind,
}

/// The plan: wave 1 merges with no refresh and no delta review; wave 2 is one refresh at a time.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub base_sha: String,
    pub wave1: Vec<u32>,
    pub wave2: Vec<Refresh>,
    pub titles: BTreeMap<u32, String>,
    /// The head commit each PR was planned against (with `base_sha`, what two runs are compared by).
    pub heads: BTreeMap<u32, String>,
    pub stacked: BTreeMap<u32, Vec<u32>>,
    /// PRs GitHub had not yet computed `mergeable` for: wave 1 may be wrong until they are read again.
    pub unknown: Vec<u32>,
}

struct Deferred {
    why: String,
    files: Vec<String>,
}

fn parents_of(before: &Before, n: u32) -> Vec<u32> {
    before.get(&n).map(|s| s.iter().copied().collect()).unwrap_or_default()
}

fn ordered(before: &Before, a: u32, b: u32) -> bool {
    parents_of(before, b).contains(&a) || parents_of(before, a).contains(&b)
}

fn paths_len(by: &BTreeMap<u32, &Pr>, n: u32) -> usize {
    by.get(&n).map_or(0, |p| p.paths.len())
}

pub fn refs(nums: &[u32]) -> String {
    nums.iter().map(|a| format!("#{a}")).collect::<Vec<_>>().join(", ")
}

// Determinism (#1320): the same base and the same PR heads give the same plan, whatever order the PRs arrive in.
// Every choice below is a `min_by_key` / `sort_by_key` whose key ends in the PR number (a total order), and every
// collection that is walked is a BTreeMap/BTreeSet (ascending PR number). The keys are named so each is pinned.

/// Wave-1 candidate: fewest live rivals, then fewest changed paths, then the lowest number.
pub(crate) fn wave1_pick_key(rivals: usize, paths: usize, n: u32) -> (usize, usize, u32) {
    (rivals, paths, n)
}

/// Merge order inside wave 1 among PRs whose parents are in: docs first, then fewest hot files, then fewest paths,
/// then the lowest number.
pub(crate) fn order_key(docs: bool, hot: usize, paths: usize, n: u32) -> (bool, usize, usize, u32) {
    (!docs, hot, paths, n)
}

/// Wave-2 order: fewest unmerged parents among the deferred, then fewest conflicted files, then the lowest number.
pub(crate) fn wave2_key(entangled: usize, files: usize, n: u32) -> (usize, usize, u32) {
    (entangled, files, n)
}

/// Of two PRs sharing unmerged history the one with fewer own commits goes first; a tie goes to the lower number
/// (`a < b`).
pub(crate) fn shared_first(a: u32, b: u32, own_a: usize, own_b: usize) -> (u32, u32) {
    if own_a <= own_b { (a, b) } else { (b, a) }
}

/// Orders the PRs. `trial` answers each merge question; the order and the wave split follow the reference planner.
pub fn plan(facts: &Facts, trial: &mut Trial<'_>) -> Result<Plan, String> {
    // Sorted by (number, head) before the map is built, so a repeated number resolves the same way every time.
    let mut listed: Vec<&Pr> = facts.prs.iter().collect();
    listed.sort_by(|a, b| (a.number, &a.head).cmp(&(b.number, &b.head)));
    let by: BTreeMap<u32, &Pr> = listed.into_iter().map(|p| (p.number, p)).collect();
    let nums: Vec<u32> = by.keys().copied().collect();
    let mut before = facts.before.clone();
    for n in &nums {
        before.entry(*n).or_default();
    }

    // Two PRs that share unmerged history: the one with fewer commits of its own on top of it goes first.
    for (i, &a) in nums.iter().enumerate() {
        for &b in &nums[i + 1..] {
            if ordered(&before, a, b) {
                continue;
            }
            // The pair is keyed (low, high); a caller that keyed it the other way round is read as (high's, low's).
            let own = facts.shared.get(&(a, b)).copied().or_else(|| facts.shared.get(&(b, a)).map(|&(x, y)| (y, x)));
            let Some((own_a, own_b)) = own else { continue };
            let (first, second) = shared_first(a, b, own_a, own_b);
            before.entry(second).or_default().insert(first);
        }
    }

    // Which PRs merge with the base on their own; the rest are refreshed later.
    let mut base_clash: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    let mut github_only: BTreeSet<u32> = BTreeSet::new();
    let mut deferred: BTreeMap<u32, Deferred> = BTreeMap::new();
    let mut ok: Vec<u32> = Vec::new();
    for &n in &nums {
        let mergeable = by.get(&n).map_or(Mergeable::Unknown, |p| p.mergeable);
        match trial(&[], n)? {
            None => match base_state(mergeable, true) {
                BaseState::GithubOnly => {
                    github_only.insert(n);
                }
                BaseState::Sim | BaseState::Conflict => ok.push(n),
            },
            Some(files) => {
                base_clash.insert(n, files);
            }
        }
    }
    for &n in &github_only {
        let bases = by.get(&n).map_or(0, |p| p.merge_bases);
        let why = format!("GitHub reports a conflict git does not ({bases} merge bases) - a refresh is a plain merge of {}", facts.base_name);
        deferred.insert(n, Deferred { why, files: Vec::new() });
    }

    // Pairwise clashes among those, each tried on top of the first of the pair.
    let mut clash: BTreeMap<u32, BTreeMap<u32, Vec<String>>> = BTreeMap::new();
    for (i, &a) in ok.iter().enumerate() {
        for &b in &ok[i + 1..] {
            if ordered(&before, a, b) {
                continue;
            }
            if let Some(files) = trial(&[a], b)? {
                clash.entry(a).or_default().insert(b, files.clone());
                clash.entry(b).or_default().insert(a, files);
            }
        }
    }

    // Wave 1 is an independent set: the PR with the fewest live rivals goes in, its rivals are deferred.
    for &n in &nums {
        if let Some(files) = base_clash.remove(&n) {
            deferred.insert(n, Deferred { why: format!("conflicts with {} now", facts.base_name), files });
        }
    }
    let mut pool: BTreeSet<u32> = ok.iter().copied().collect();
    let mut chosen: Vec<u32> = Vec::new();
    loop {
        let pick = pool.iter().copied().min_by_key(|x| {
            let rivals = clash.get(x).map_or(0, |m| m.keys().filter(|c| pool.contains(c)).count());
            wave1_pick_key(rivals, paths_len(&by, *x), *x)
        });
        let Some(n) = pick else { break };
        pool.remove(&n);
        chosen.push(n);
        for (c, files) in clash.get(&n).cloned().unwrap_or_default() {
            if pool.remove(&c) {
                deferred.insert(c, Deferred { why: format!("conflicts with #{n}"), files });
            }
        }
    }

    // A PR whose parent is not in wave 1 cannot be either.
    let mut changed = true;
    while changed {
        changed = false;
        for n in chosen.clone() {
            let missing: Vec<u32> = parents_of(&before, n).into_iter().filter(|a| !chosen.contains(a)).collect();
            if !missing.is_empty() {
                chosen.retain(|x| *x != n);
                deferred.insert(n, Deferred { why: format!("its parent {} is not in wave 1", refs(&missing)), files: Vec::new() });
                changed = true;
            }
        }
    }

    // Parents first; then docs before code, cool before hot, small before large.
    let mut order: Vec<u32> = Vec::new();
    let mut left: BTreeSet<u32> = chosen.iter().copied().collect();
    while !left.is_empty() {
        let ready: Vec<u32> = left
            .iter()
            .copied()
            .filter(|n| !parents_of(&before, *n).iter().any(|a| left.contains(a)))
            .collect();
        let candidates: Vec<u32> = if ready.is_empty() { left.iter().copied().collect() } else { ready };
        let pick = candidates.into_iter().min_by_key(|x| {
            let p = by.get(x);
            order_key(p.is_some_and(|p| p.docs()), p.map_or(0, |p| p.hot()), paths_len(&by, *x), *x)
        });
        let Some(n) = pick else { break };
        left.remove(&n);
        order.push(n);
    }

    // Wave 1: each merge is checked on top of the ones before it. A child of a PR that did not make it is deferred too.
    let mut wave1: Vec<u32> = Vec::new();
    for n in order {
        let missing: Vec<u32> = parents_of(&before, n).into_iter().filter(|a| !wave1.contains(a)).collect();
        if !missing.is_empty() {
            deferred.insert(n, Deferred { why: format!("its parent {} is not in wave 1", refs(&missing)), files: Vec::new() });
            continue;
        }
        match trial(wave1.as_slice(), n)? {
            None => wave1.push(n),
            Some(files) => {
                deferred.insert(n, Deferred { why: "conflicts once the earlier wave-1 PRs are in".into(), files });
            }
        }
    }

    // Wave 2: the least entangled first, each checked against the main that wave 1 leaves.
    let mut keys: Vec<u32> = deferred.keys().copied().collect();
    let in_deferred: BTreeSet<u32> = keys.iter().copied().collect();
    keys.sort_by_key(|x| {
        let entangled = parents_of(&before, *x).iter().filter(|a| in_deferred.contains(a)).count();
        wave2_key(entangled, deferred.get(x).map_or(0, |d| d.files.len()), *x)
    });
    let mut wave2: Vec<Refresh> = Vec::new();
    for n in keys {
        let Some(d) = deferred.remove(&n) else { continue };
        let (clean, mut files) = match trial(wave1.as_slice(), n)? {
            None => (true, Vec::new()),
            Some(f) => (false, f),
        };
        if files.is_empty() {
            files = d.files;
        }
        let kind = if clean && github_only.contains(&n) {
            Kind::PlainRefresh
        } else if clean {
            Kind::CleanAfterWave1
        } else if !files.is_empty() && files.iter().all(|f| mechanical(f)) {
            Kind::Mechanical
        } else {
            Kind::ByHand
        };
        wave2.push(Refresh { number: n, why: d.why, files, kind });
    }

    let titles = by.iter().map(|(n, p)| (*n, p.title.clone())).collect();
    let heads = by.iter().map(|(n, p)| (*n, p.head.clone())).collect();
    let stacked = nums
        .iter()
        .filter_map(|n| {
            let parents = parents_of(&before, *n);
            (!parents.is_empty()).then_some((*n, parents))
        })
        .collect();
    let unknown = nums.iter().copied().filter(|n| by.get(n).is_some_and(|p| p.mergeable == Mergeable::Unknown)).collect();
    Ok(Plan { base_sha: facts.base_sha.clone(), wave1, wave2, titles, heads, stacked, unknown })
}

