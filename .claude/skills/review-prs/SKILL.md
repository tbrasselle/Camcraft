---
name: review-prs
description: Find open pull requests on NeoTech/Camcraft that need a review (third-party PRs from forks, or any PR whose current head has not been reviewed yet) and dispatch the PR-reviewer agent on each, one at a time. Posts review COMMENTS only - never approves or merges. Use when asked to "review the PRs", "check for new pull requests", "review PR #N", or from the periodic loop (`/loop 10m /review-prs`).
---

# Review open pull requests

The `PR-reviewer` agent (`.claude/agents/PR-reviewer.md`) does the review of
one PR. This skill decides which PRs need it and dispatches it. The maintainer
approves and merges by hand - nothing here approves, merges or pushes.

## 0. One review at a time - the lock

PR reviews never run concurrently (user, 2026-11-07: "make sure we are not
running concurrent PR reviews"): each one builds the whole workspace, and a
loop tick can fire while the previous review is still building. The lock is
the file `.claude/pr-review.lock` in the repo root (gitignored), one line:
`<PR number> <head sha> <UTC ISO time>`.

```bash
lock="$(git rev-parse --show-toplevel)/.claude/pr-review.lock"
if [ -f "$lock" ]; then cat "$lock"; find "$lock" -mmin +90; fi
```

- Lock present and younger than 90 min (the `find` prints nothing): a review
  is running. Say "a PR review is already running (#N since T)" in one line
  and stop - do not dispatch, do not queue.
- Lock older than 90 min: stale (a review that died). Check no
  `../cc-pr*-review` worktree is still building (`git worktree list`); if none,
  delete the lock, say "removed a stale PR-review lock (#N)", and go on.
  If a review worktree remains, remove it (`git worktree remove --force`,
  `git branch -D pr-<N>-review`) before going on.
- Also stop if a `PR-reviewer` agent you launched is still running in this
  session.

## 1. Find PRs that need a review

**Only PRs marked ready for review are reviewed - never a draft** (user,
2026-11-08: "Update the review-prs skill to only review PRs that is ready
to review"; AGENTS.md "Every PR opens as a draft"). A draft is still being
worked on and may take more pushes; reviewing it wastes a full build.

If the user named a PR (`/review-prs 58`), check it first:
`gh pr view <N> --json isDraft,state --jq '.isDraft, .state'`. If it is a
draft (or not open), say "PR #N is a draft - not reviewed until it is marked
ready (`gh pr ready N`)" in one line and stop. Otherwise take just that one
and go to step 2.

Otherwise list open PRs that are ready for review (non-draft) with their head SHA and reviews:

```bash
gh pr list --repo NeoTech/Camcraft --state open --limit 100 \
  --json number,isDraft,author,headRepositoryOwner,headRefOid,reviews,comments \
  --jq '.[] | select(.isDraft|not)
        | .headRefOid as $s
        | select([.reviews[].body, .comments[].body] | map(select(contains("<!-- pr-reviewer: " + $s))) | length == 0)
        | "\(.number) \(.author.login) \($s)"'
```

Every line printed is a PR that needs a review:
- it is ready for review - a fork PR or one of our own (user, 2026-11-08:
  "All PR*s made by owner should be reviewd and tagged"); our own PRs are
  flagged ready by a quick pass (AGENTS.md "Every PR opens as a draft") and
  get their full review here;
- no review or comment carries the marker for its current head SHA (a new push
  after our last review means the contributor answered - review again).

Nothing to review: say "no PRs need review" in one line and stop (this is the
common case on a loop tick - keep it quiet).

## 2. Dispatch exactly one PR

Take the oldest PR needing review - **one per invocation**; the next loop tick
takes the next. Never launch two `PR-reviewer` agents at once.

1. Write the lock first:
   `echo "<N> <sha> $(date -u +%Y-%m-%dT%H:%MZ)" > "$(git rev-parse --show-toplevel)/.claude/pr-review.lock"`
2. Agent tool, `subagent_type: "PR-reviewer"`, run in the background, prompt:
   `Review PR #<N> of NeoTech/Camcraft (head <sha>). Follow your definition: comment only, own worktree, clean up, remove .claude/pr-review.lock when done.`
3. On its completion notification, make sure the lock is gone (delete it if
   the agent failed to) and that `../cc-pr<N>-review` is removed. Do not poll
   while it runs.

## 2b. Labels - one colour per state

Every reviewed PR carries exactly the labels its state calls for (user,
2026-11-08: "a merge conflict should prob get orange flagged 'conflicts' so
we can find em"):

| Label | Colour | Means |
|---|---|---|
| `merge-ready` | green | review clean AND merges cleanly with main - the owner can merge now |
| `needs-rework` | red | the review found blocking items |
| `conflicts` | orange | conflicts with main - needs a branch refresh (`git merge origin/main`) |
| `needs-maintainer` | yellow | reviewed, no blockers, but the owner must decide first: a new dependency, or a `build.rs` / scripts / `.github` change the reviewer reads and does not build ("Not built - needs a maintainer look") |

`needs-maintainer` (user, 2026-11-09: "that needs-maintainer needs to
into the skill right?") gives the "Not built - needs a maintainer look"
verdict a label. Without it such a PR carried none and looked unreviewed
on the PR list. The owner removes it after deciding: on a yes it becomes
`merge-ready`, or is built and reviewed in full first; on a no it is
closed or gets `needs-rework`.

After posting the review, read `gh pr view <N> --json mergeable`
(re-read once if it says `UNKNOWN`):
- clean review + `MERGEABLE` -> add `merge-ready`, remove `needs-rework`, `conflicts` and `needs-maintainer`;
- clean review + `CONFLICTING` -> add `conflicts`, remove `merge-ready` and `needs-maintainer` (never green while it conflicts);
- blocking review -> add `needs-rework`, remove `merge-ready` and `needs-maintainer`; also add `conflicts` if it conflicts;
- "Not built - needs a maintainer look" with no blockers -> add `needs-maintainer`, remove `merge-ready` and `needs-rework`; also add `conflicts` if it conflicts. With blockers it is `needs-rework` as above, and the comment still names the owner's decision.

A PR that changes `crates/ui/tests/snapshots/*.png` is a blocking review
(`needs-rework`): the pixel goldens are release-lane and are regenerated
only at the cut (AGENTS.md "Pixel snapshots are release-lane", #255). The
fix is to drop the PNG changes and update the structural assertions
instead.

A red or orange verdict also turns the PR back into a draft
(`gh pr ready <N> --undo`): the author fixes it there, pushes, and flags it
ready again for a fresh review (AGENTS.md "Every PR opens as a draft").
That applies to our own PRs; a fork contributor's PR is left as they set
it, and the review comment says what to fix.

Whoever refreshes a branch re-reads `mergeable` after the push: once it is
`MERGEABLE`, drop `conflicts` and restore `merge-ready` if the last review
was clean. Merging a PR can turn others `CONFLICTING`, so each run also
re-checks every open PR labelled `merge-ready` and swaps any that now
conflict to `conflicts`.

## 2c. Token cost comment - when a PR turns merge-ready

When a review ends with the PR labelled `merge-ready` (step 2b), the
dispatcher (this skill's caller, not the reviewer - only the caller sees
the agent's usage) posts one more PR comment with the estimated token
cost of getting that PR to merge-ready (user, 2026-11-10: "an average
token cost should be added as comment to each review based when its
merge ready").

- **What counts:** every agent run on this PR that the caller dispatched,
  plus the merge-ready review itself - each review and re-review
  (`PR-reviewer`), conflict-resolution or replacement-branch agents, and
  the fix agent for our own issue PRs. Take each run's token total from
  its completion notification (`subagent_tokens`) and its model. The
  caller's own main-session turns aren't attributable per PR and are not
  counted - say so.
- **Rate:** usage reports one total, not an input/output split, so
  estimate it as 90 % input and 10 % output at list price - a blended
  upper estimate (cache reads, billed at about a tenth of input, make the
  real cost lower). Blended per million tokens: Opus 5.5 about $5.60,
  Sonnet 5.5 about $2.80, Haiku 5.5 about $0.14. When list prices change,
  update these numbers from the `claude-api` skill's model table.
- **Comment shape** (one comment, first line is the marker so it's never
  posted twice for the same head):

  ```
  <!-- review-cost: <head sha> -->
  **Estimated agent cost to merge-ready:** ~$X.XX (N tokens)
  | Run | Model | Tokens | Est. cost |
  |---|---|---|---|
  | review | Sonnet 5.5 | 71k | ~$0.20 |
  | conflict fix (replacement #M) | Opus 5.5 | 64k | ~$0.36 |
  Blended upper estimate (90 % input / 10 % output at list price; cache
  reads make the real cost lower). Main-session coordination isn't
  counted.
  ```
- A run whose token total isn't known (e.g. a review from another
  session) is listed as "not known" and left out of the sum.
- No cost comment for `needs-rework`, `conflicts` or `needs-maintainer`
  verdicts; the next review that ends `merge-ready` sums every run on
  the PR so far (earlier rounds included).

## 2d. Process PRs - version bump and announcement

A PR that changes a process file (`AGENTS.md`, `.claude/agents/`,
`.claude/skills/`, `scripts/gate.ps1`, `scripts/gate.sh`) must increment
the integer in `.claude/PROCESS_VERSION` (AGENTS.md "Process changes bump
`PROCESS_VERSION`"). The reviewer checks this; a missing bump is a
requested change (`needs-rework`). When such a PR merges, whoever merges
it comments on the pinned issue #681 (Process changes):
`PROCESS_VERSION <n>` · PR #M · what changed · what a fork must do (usually
"pull the process files", the command in the issue body). That comment is
how fork maintainers subscribed to #681 hear about it.

## 3. Report

One short line per PR: number, author, verdict, link to the posted review.
If a review could not be posted, say why. Do not merge or approve, and set no labels beyond step 2b's review labels; do not
close anything; do not reply on the contributor's behalf.

## 4. Mirror claim comments - add and withdraw

A claim comment is a claim (AGENTS.md "A claim comment is a claim"): only
collaborators can set labels, so anyone else claims by commenting a line
that starts `claimed:` or `/claim`. The label only mirrors the comment. On
every run, after the PR steps, sync the two both ways.

**Add.** Find claim comments by anyone but the owner account (agents post
as the owner):

```bash
since=$(date -u -d '-7 days' +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -u -v-7d +%Y-%m-%dT%H:%M:%SZ)
gh api "repos/NeoTech/Camcraft/issues/comments?since=$since&per_page=100" --paginate \
  --jq '.[] | select((.body|test("(^|\n) *(claimed:|/claim)"; "i")) and (.user.login != "NeoTech"))
        | "\(.issue_url|split("/")|last) \(.user.login) \(.created_at)"'
```

For each line `<N> <login> <time>`:
- **withdrawn** - `<login>` commented `unclaim` (or `/unclaim`) after
  `<time>`: skip silently. A withdrawn claim's old comment is never
  mirrored again (without this the Withdraw pass and this one would flip
  the label every run);
- **paused issue** (labelled `P?` - AGENTS.md "`P?` is paused"): do not
  mirror; reply once (only if no later comment mentions `@<login>`)
  `This issue is paused by the maintainer (P?) - it isn't open for work yet; the claim isn't recorded. It will be once the maintainer sets a priority.`;
- **closed issue**, or **already mirrored** (`claimed` set and `<login>`
  assigned, or `claimed` set and a "Claim recorded for @<login>" reply
  exists): skip silently;
- **held by someone else** - an earlier claim comment by another
  contributor still exists, or a fresh agent claim (4 h rule): do not
  mirror; reply once (only if no later comment mentions `@<login>`) naming
  the current claimant;
- **otherwise**: `gh issue edit N --add-label claimed --add-assignee <login>`
  (if GitHub refuses the assignee, add the label alone) and reply
  `Claim recorded for @<login> - label and assignee set (only collaborators can set labels; your comment is the claim).`
  A contributor's claim does not go stale on a timer; it stands while the
  comment exists.

**Withdraw.** For every open issue carrying `claimed`
(`gh issue list --state open --label claimed --limit 200`), look for the
claim behind it: a claim comment by a non-owner (or an agent's `claimed:`
comment, which follows the 4 h rule instead). If the only claim behind the
label was a contributor's comment that is now deleted, or that contributor
later commented `unclaim`, remove the label and their assignment
(`gh issue edit N --remove-label claimed --remove-assignee <login>`) and
comment `claim by @<login> withdrawn (comment removed) - issue is free again`.
A label with no findable claim of any kind and no "Claim recorded" reply
was set by a collaborator by hand: leave it.

Report one line per mirrored or withdrawn claim; nothing found stays silent.

## Periodic run

`/loop 10m /review-prs` re-runs this every 10 minutes for the session
(recurring loops auto-expire after 7 days; cancel with CronDelete). Each tick is
a cheap `gh pr list`; an agent is only spawned when a fork PR has an
unreviewed head.
