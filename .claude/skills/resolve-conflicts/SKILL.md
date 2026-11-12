---
name: resolve-conflicts
description: Re-check open pull requests that conflict with main and get them mergeable again. Contributor mode (`/resolve-conflicts` or `/resolve-conflicts <login>`) - for the given author (default the gh user), finds their open PRs that conflict with upstream main, merges main into each branch, resolves the usual easy conflicts (both sides added lines, module lists, tests, snapshot PNGs), runs the targeted gate, pushes, and comments that the PR is ready for a re-check. Maintainer mode (`/resolve-conflicts check`) - re-reads every PR labelled `conflicts`, drops the label from those that merge cleanly again, and routes each to the review it needs. Use for "fix my conflicts", "rebase my PRs on main", "which PRs still conflict", "did the contributors resolve their conflicts".
---

# resolve-conflicts - get conflicting PRs mergeable again

Part of AGENTS.md's Autonomous Work Law ("Conflicted PRs are re-merged by
their author; the maintainer re-checks"). Merging other PRs moves `main`,
and a reviewed PR that merged cleanly yesterday can conflict today. Most
of these conflicts are mechanical: both sides added an entry to the same
module list, test list, match or file. This skill fixes those, and stops
and reports when the two sides really clash.

Repository: NeoTech/Camcraft (`gh repo view --json nameWithOwner`; in a
fork, `upstream` is NeoTech/Camcraft and `origin` is the fork).

**Two modes:**
- `/resolve-conflicts [login]` - **contributor mode**: fix *your* PRs (or
  `login`'s, if you can push to their branches).
- `/resolve-conflicts check` - **maintainer mode**: re-check every PR
  labelled `conflicts` and update its state.

## 0. Preconditions

- Never during a release cut (an open PR from a `release/*` branch): stop
  and say so.
- Never force-push and never rebase. Always `git merge` (merge commits keep
  the hashes reviews and comments refer to).
- Never build in the main checkout. Work in a worktree under
  `.claude/worktrees/` with its own `target/` (the worktree default, no `CARGO_TARGET_DIR` env var - it defeats the shared compile cache); remove both when done.
- Build cache: Prefer `cargo xtask gate ...` (it turns the compile cache on); for a direct cargo call set `RUSTC_WRAPPER=sccache` only when `sccache` is on PATH (cargo fails if the wrapper is missing), and never export `CARGO_TARGET_DIR`.
- A PR flagged ready and labelled `merge-ready` takes no pushes (AGENTS.md
  "Every PR opens as a draft"). A conflicting PR has already lost
  `merge-ready` (the label is `conflicts`), so pushing a merge to it is
  allowed. If the PR is still marked ready (ours or a contributor's), run
  `gh pr ready <N> --undo` before pushing and say why in a PR comment. A
  contributor can do this on their own PR; when you can't (someone else's
  PR you only have push access to), say in the comment that it should be
  re-flagged ready after the re-check.

## 1. Contributor mode - `/resolve-conflicts [login]`

### 1a. Find the PRs

```bash
me=${LOGIN:-$(gh api user --jq .login)}
gh pr list --repo NeoTech/Camcraft --state open --author "$me" --limit 100 \
  --json number,title,headRefName,headRepositoryOwner,isDraft,mergeable,labels
```

Re-read `mergeable` for each (retry while `UNKNOWN`, up to ~6 tries). Work
on the ones that are `CONFLICTING` or carry the `conflicts` label. Order
them so stacks go parent first: if PR B contains PR A's commits (A's head
is an ancestor of B's head, `git merge-base --is-ancestor`), fix A first,
then merge A's fixed branch into B before main.

### 1b. For each PR

1. **Worktree on the PR branch** (your fork's branch):
   `git fetch upstream` (or `origin` in the canonical repo), then
   `git worktree add .claude/worktrees/rc-<N> <branch>`.
2. **Merge main:** `git merge upstream/main` (or `origin/main`).
3. **Resolve, by kind:**
   - **Both sides added** (module lists in `lib.rs`/`mod.rs`, test module
     lists, `pub mod` lines, enum variants, match arms, struct fields,
     separate functions, separate tests, menu items, machine-file entries):
     keep both, in a sensible order. This is the common case.
   - **Snapshot PNGs** (`crates/ui/tests/snapshots/*.png`): take main's.
     While merging main into the PR branch, "theirs" is main:
     `git checkout --theirs -- crates/ui/tests/snapshots` (or
     `git checkout upstream/main -- crates/ui/tests/snapshots`). A PR never
     changes a snapshot PNG (AGENTS.md "Pixel
     snapshots are release-lane"). Update the structural assertions
     instead if the UI changed.
   - **`CHANGELOG.md` / `RECOMMENDED.md`:** a PR shouldn't touch these
     (the cut writes them). Take main's and drop the PR's edit.
   - **`.claude/PROCESS_VERSION`:** take the higher number, then add 1 if
     this PR changes process files and main already used that number.
   - **The same logic changed differently on both sides** (two rewrites of
     one function body, two different fixes of one check, a refactor that
     moved code main edited in place): **stop.** Don't guess. `git merge
     --abort`, leave the branch untouched, and report the hunk: file,
     function, what each side does. A rule that keeps safety
     checks (per-setup spindle, carried stock, refusals by name) must never
     be dropped by a resolution - when in doubt, it's this case.
4. **Check nothing from main was dropped:** `git diff upstream/main --stat`
   must list only the PR's own files.
5. **Targeted gate** (AGENTS.md "Test the change"): `cargo xtask gate
   quick --base upstream/main` on any OS (wrappers `scripts/gate.ps1` /
   `gate.sh` forward to it), or by hand: build the touched
   crates; run the test binaries of the files that conflicted plus the
   PR's own pinning tests; `cargo clippy -p <touched crates> --all-targets
   -- -D warnings`. UI tests with CWD `crates/ui`. Run in the foreground
   with timeouts. Red -> fix the resolution if it's yours; if the failure is
   in main's code or the PR's logic, stop and report.
6. **Commit** the merge: `Merge main into <branch> (<files>: keep both
   sides)` (plus `Co-Authored-By` if an agent did it). **Push** plainly
   (`git push origin <branch>` - never `--force`).
7. **Comment on the PR:** `Merged main to resolve conflicts (<files>: keep
   both sides). Targeted gate: <results>. Ready for a re-check
   (/resolve-conflicts check).` Contributors can't change labels; the
   maintainer's check does that.
8. Remove the worktree and its target (cd out first).

### 1c. Report

One line per PR: merged & pushed (with the gate result), or stopped (with
the hunk that needs a human decision), or still conflicting after the
push (rare - main moved again; run once more).

## 2. Maintainer mode - `/resolve-conflicts check`

For every open PR labelled `conflicts` (plus any open PR that GitHub now
reports `CONFLICTING` without the label - add the label to those):

1. Re-read `mergeable` (retry while `UNKNOWN`).
2. **Still `CONFLICTING`:** keep `conflicts`. If no comment since the label
   was set asks the author to merge main, post one (once):
   `This conflicts with main. Please run /resolve-conflicts (or git merge
   upstream/main, keep both sides) and push. The usual cause is that both
   sides added lines in the same place.` If the author's fork refuses
   maintainer pushes and the maintainer wants it merged now, the
   replacement-branch route applies (fix on a branch in our repo, PR with
   their commits kept; GitHub then shows the original PR as merged).
3. **Now `MERGEABLE`:** remove `conflicts`, then decide what review it needs:
   - **Head unchanged since the last review** (main moved, the conflict went
     away by itself): restore `merge-ready` if that review was clean.
   - **Head moved, and the only new commits are a merge of main whose diff
     against main is just the PR's own files** (the author's
     resolve-conflicts push): a **delta review** - a reviewer checks only
     the merge commit (both sides kept, nothing from main dropped, targeted
     tests). Clean -> `merge-ready`, with the reviewer marker for the new
     head. The delta review sets any of the review-prs labels (step 2b):
     `needs-rework` if the resolution is wrong, `conflicts` if main moved
     again, and `needs-maintainer` if the merge brought in a dependency,
     `build.rs`, scripts or `.github` change the reviewer only reads.
   - **Head moved with new code commits:** a full review via `/review-prs`.
4. Report one line per PR: still conflicting (author asked), clean again
   (label restored), sent to delta review, sent to full review.

## 3. Notes

- This skill never merges a PR into main; merging stays with `/merge-ready`
  or the maintainer.
- Contributors running it from a fork use their own `gh` login; the
  maintainer runs it from the canonical repo.
- Changing this skill is a process change: bump `.claude/PROCESS_VERSION`
  and announce it on the pinned issue #681.
