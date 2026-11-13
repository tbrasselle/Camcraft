---
name: merge-ready
description: Merge the open pull requests labelled `merge-ready`, after asking the user. Lists every open `merge-ready` PR, plans the merge order with `cargo xtask merge-plan` (so as few PRs as possible need a refresh and a delta review), writes a 2-line summary of each, asks yes/no in groups of 2-4 per prompt, then merges the approved ones as one batch - a clean wave first, then the conflicting ones refreshed one at a time. Answers of "no" leave the PR open. Use when the user says "/merge-ready", "merge the ready PRs", "go through the merge-ready PRs", "which PRs can we merge", or asks for a merge order.
---

# merge-ready - approve and merge the `merge-ready` PRs

Part of the repository (`.claude/skills/merge-ready/`), so every agent and
every fork runs the same procedure and gets the same order. It needs only
`gh`, `git` and `cargo xtask merge-plan` (AGENTS.md "Refresh only at merge
time").

**Who runs what** (maintainer, 2026-11-12: "Both collaborators"): any
collaborator's session (an account with write access to the repository) runs
all of it, merges included. Release cuts and tags stay with the maintainer.
Anyone else may run steps 0-2 read-only (collect and plan) to see the plan for
their own PRs; they never merge or relabel.

The user decides; this skill summarises, asks, then merges exactly what was
approved. Never merge a PR the user didn't say yes to, and never approve a PR
(`gh pr review --approve`).

## Why the order matters

Every merge moves `main`. A PR that then conflicts must take a merge of
main: its head moves, its review no longer names its head, and it needs a
delta check - and the next merge can conflict it again. Merging ten PRs one
by one, in any order, can cost twenty refreshes. Three rules keep that near
zero:

1. **Batch, don't trickle.** Collect all approvals first, then merge the
   whole set in one run, in the planned order.
2. **A PR that merges cleanly needs nothing.** GitHub's merge commit of an
   unchanged, reviewed head is not a new head: no refresh, no delta. Merge
   the largest cleanly-merging set first (wave 1).
3. **Refresh late, once, one at a time.** A conflicting PR is refreshed only
   when it is the next one to merge, against the main of that moment, then
   merged before anything else is refreshed (wave 2). Never refresh a PR that
   is not next in the batch: the next merge to main throws that refresh away.

The planner is deterministic: the same base and the same PR heads give the
same plan on every machine, in the same order (ties end in the PR number).
The plan prints the base sha and each PR's head sha; two sessions comparing
plans compare those.

## 0. Preconditions - stop and say why if any fails

- **No release cut in progress.** An open PR from a `release/*` branch
  (`gh pr list --state open --search "head:release/"`) means nothing merges
  to main until the tag (AGENTS.md "A cut goes PR review -> merge to main -> tag").
- **Repo rules win.** Read AGENTS.md's merge rules (merge commits only,
  `--delete-branch` for our own branches, feat-at-cut, stacked PRs, the draft
  rules) and follow them where they differ from this skill.
- **Never touch another session's checkout.** Run `git fetch` only; the
  planner needs no checkout. Merging happens through `gh pr merge`.

## 1. Collect

```bash
export COLLABS=$(gh api repos/NeoTech/Camcraft/collaborators --jq '.[] | select(.permissions.push) | .login' | tr '\n' ' ')
gh pr list --repo NeoTech/Camcraft --state open --label merge-ready --limit 100 \
  --json number,title,author,isDraft,headRefName,headRefOid,baseRefName,headRepositoryOwner,mergeable,files,body,labels,reviews,comments
```

For each PR re-read `mergeable` (`gh pr view N --json mergeable`; GitHub
answers `UNKNOWN` for a few seconds after main moves, so retry up to ~6 times
with a short wait).

**Only collaborators' reviews count** (AGENTS.md "Only collaborators'
reviews count"). A review marker (`<!-- pr-reviewer: <sha> -->`) counts only
in a comment or review by an account in `$COLLABS`, and a `merge-ready` label
counts only if a collaborator set it (`gh api repos/NeoTech/Camcraft/issues/N/events`,
last `labeled` event for `merge-ready`). A contributor's agents can post the
same marker and, with label rights, the same label on their own PR; that is
the author's claim, not a review. Such a PR is reported ("#N: merge-ready was
set by a non-collaborator, no review from us for this head") and left out.
A collaborator's own PR counts like any other, provided an agent that did not
write it did the review.
A PR whose current head carries a collaborator review that requests changes
and also a `merge-ready` label (or `merge-ready` beside `needs-rework`) is a
disagreement (AGENTS.md "Only collaborators' reviews count"): report it,
leave it out, and change neither label.

Drop a PR from the list, with a one-line note to the user, when:
- it is a **draft** (a ready one is required for merging);
- it carries **`needs-rework` or `needs-maintainer`** alongside `merge-ready`
  (contradictory labels): report it rather than ask about it;
- its **head moved since the collaborator's review** and the move is more than
  a clean merge of main. Find the reviewed head (the newest collaborator-authored
  marker) and classify the move:

  ```bash
  cargo xtask merge-plan --delta N --reviewed <reviewed sha>
  ```

  | Result | Means | Do |
  |---|---|---|
  | `SAME` | only merges of main, no hand edits (empty `--remerge-diff`): the tree is exactly what merging the reviewed head would give | the review stands - keep the PR in the list, no agent |
  | `MECHANICAL` | hand edits only in rule-settled files (`.claude/PROCESS_VERSION`, `CHANGELOG.md`, `RECOMMENDED.md`, `Cargo.lock`, snapshot PNGs) | read `git show --remerge-diff <merge>` yourself (a few lines); keep the PR if it follows the rule, no agent |
  | `DELTA` | a conflict was resolved by hand | leave it out: "PR #N needs a delta review of its merge" (read-only, the remerge-diff only) |
  | `NEW CODE` | new commits, a merged branch that is not main, an octopus merge, or rewritten history | leave it out: "PR #N has new commits since its review" |

A PR that is **`CONFLICTING`** but otherwise review-clean is NOT dropped: it
goes to wave 2 (step 2). Swap its label `merge-ready` for `conflicts` only if
it ends the run unmerged.

If nothing is left, say "no PRs are merge-ready" and stop.

## 2. Plan the order - simulate, don't guess

File overlap is a poor predictor (two PRs can touch one file without
conflicting, and `mod.rs` lists conflict all the time). Simulate instead; it
takes seconds and builds nothing:

```bash
cargo xtask merge-plan <N> <N> ...      # the PRs left after step 1
cargo xtask merge-plan --label merge-ready   # or every open PR with the label
cargo xtask merge-plan --json <N> <N> ...    # the same plan for other tools
```

It fetches each head, merges them in memory with `git merge-tree` (nothing is
checked out, built or pushed), and prints the base sha, then:

- **Wave 1** - the largest set it found that merges cleanly one after the
  other, in order: stack parents before children (a child's base is the
  parent's branch, or its history contains the parent's commits),
  docs/process-only first, small before large, hot files (`lib.rs`, `mod.rs`,
  `state.rs`, `schema.rs`, `Cargo.*`, `AGENTS.md`) last. None of these needs
  a refresh or a delta review.
- **Wave 2** - every PR that conflicts with main now or with a wave-1 PR, with
  the conflicting files and a kind: `clean after wave 1`, `mechanical` (only
  rule-settled files) or `by hand`.

  A `plain refresh` entry is a PR git merges cleanly but GitHub reports as
  `CONFLICTING` (criss-cross history, e.g. a stack whose parent took a commit
  after the child branched). GitHub will not merge it as it is; its refresh is
  a merge of main with no hand edits, which `--delta` classes `SAME` - the
  review stands.
- A `WARNING` line means GitHub had not computed `mergeable` for some PRs; run
  the plan again before trusting wave 1.

Read the plan before asking:

- When two PRs conflict **with each other**, only one can be in wave 1. The
  planner keeps the one that conflicts with fewer others. If the other matters
  more (a fix over a feat, a `P1`, the user's stated priority), run the plan
  again without the one to defer, and say so.
- A stacked child whose base is not `main` is retargeted
  (`gh pr edit B --base main`) before its parent merges with
  `--delete-branch`; otherwise GitHub closes it.
- Process PRs each bump `.claude/PROCESS_VERSION` and so conflict with each
  other on that one line. That is `mechanical`: the later one takes the next
  number in its refresh.

Show the plan in two lines before asking ("Wave 1 (no refresh needed): #A, #B,
#C. Wave 2 (one refresh each, in this order): #D - conflicts with #B in x.rs;
#E").

## 3. Ask, in groups

Ask with `AskUserQuestion`, at most **4 questions per prompt**. Use groups of
2-4, in merge order. Each question:

- **Text:** `PR #N (<author>, <type>): <2-line summary - what it changes and
  why it matters>. <one risk/caveat line if any: breaking change, project-file
  version bump, changes posted G-code, not hand-tested, stacked on #M, a new
  dependency>. <"Merges cleanly." or "Needs one refresh after #X (conflict in
  <file>).">. Merge #N?`
- **Header:** `#N`.
- **Options:** `Yes, merge` / `No` (exactly these two).

Write the summary from the PR body plus the latest collaborator review
comment, not from the title alone. Say plainly if a PR changes machine output
(G-code) or the project file format, or adds a dependency. Never put a
closing keyword ("fixes #N") in your own text.

Collect all answers first (all groups), then merge. Don't merge between
prompts. If a "no" removes a PR that others were ordered after (a stack
parent, or the wave-1 side of a conflicting pair), run the plan again on the
approved set before merging: the plan changes.

## 4. Merge the approved ones, as one batch

**Wave 1**, in the planned order. For each PR:

1. Retarget any stacked child of this PR to `main` first.
2. Make sure the PR is ready (`gh pr ready N` if it is a draft that the
   rules allow to flag; AGENTS.md "Every PR opens as a draft"), then merge
   with a **merge commit** (never squash or rebase):
   - our own branch (head repository = the base repository):
     `gh pr merge N --merge --delete-branch`;
   - a fork PR: `gh pr merge N --merge` (the fork's branch is not ours to
     delete).
3. Confirm `gh pr view N --json state` is `MERGED`; note the merge commit.
4. About 5 seconds, then re-read the next PR's `mergeable` (retry while
   `UNKNOWN`). The simulation said it is clean; if GitHub says `CONFLICTING`
   anyway, something else reached main - move that PR to the end of wave 2 and
   go on.
5. A **breaking** PR (title with `!`, or a project-file version that older
   builds refuse) gets a PR comment noting that the next release notes must
   list it as breaking, if its merge commit has no `BREAKING CHANGE:` footer.

Do not re-review, re-gate or refresh anything between wave-1 merges.

**Wave 2**, strictly one PR at a time, in the planned order:

1. Refresh it against the main of this moment: our own branch - merge
   `origin/main` into it in a worktree (never rebase or force-push; never in
   a checkout another session is using), by a subagent where the repo's rules
   say side work rides subagents (`/resolve-conflicts` contributor mode is the
   tool); a fork PR - only where the PR allows maintainer edits and the user
   approved pushing there, otherwise ask the author once and leave it out of
   this run.
2. Classify the refresh with `cargo xtask merge-plan --delta N --reviewed <reviewed sha>`:
   `SAME` or `MECHANICAL` -> merge it now (AGENTS.md says which PR comment to
   leave and when the PR is flagged ready again); `DELTA` -> one read-only
   review of the remerge-diff (rebuild and run the touched tests only if logic
   was merged by hand), then merge.
3. Merge it **before** refreshing the next PR. Two refreshed PRs waiting at
   once is exactly the churn this skill avoids.
4. A PR whose refresh cannot be done in this run ends it labelled `conflicts`
   (not `merge-ready`), with one comment saying what it conflicts with.

**After all merges:**
- For each merged PR, read the issues it closed (`closingIssuesReferences`)
  and remove `in-review` and `claimed` from those issues.
- **Process files:** if a merged PR changed process files (`AGENTS.md`,
  `.claude/agents/`, `.claude/skills/`, `scripts/gate.*`, `xtask/`), post the
  `PROCESS_VERSION <n>` · PR #M · what changed · what a fork must do comment
  on the pinned issue #681, as AGENTS.md says.
- **Don't refresh the rest.** Open PRs this batch turned `CONFLICTING` and
  that are not about to merge are left alone (label `conflicts` if they were
  `merge-ready`). They are refreshed when their turn to merge comes.

For each PR answered **no**, leave it open and unchanged (labels too); just
list it in the report.

## 5. Report

One table:

| PR | Answer | Wave | Result |
|---|---|---|---|
| #N title | yes | 1 | merged `abc1234` (closes #I) |
| #M title | yes | 2 | refreshed (`MECHANICAL`), merged `def5678` |
| #L title | yes | 2 | not merged: needs the author's refresh (labelled `conflicts`) |
| #K title | no | - | left open |

Then one line per item that needs follow-up: PRs left out at step 1 and why
(non-collaborator `merge-ready`, new code since the review, needs a delta
review); PRs that ended `conflicts`; breaking changes for the next release
notes; how many refreshes and delta reviews the batch cost (the number this
skill is meant to keep down).

**One gate per batch** (AGENTS.md "Fixes are tested in the background"): when
two or more code PRs merged, dispatch one gate-runner on `origin/main` after
the run (batch tier, in the main checkout only when it is clean and idle),
never one per merge, and do not wait for it.
