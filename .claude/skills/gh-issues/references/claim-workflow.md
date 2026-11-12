# A complete claim / close workflow (default when the repo defines none)

Use this when several people or agents work one backlog and the repo has no
issue law of its own. If the repo has one (AGENTS.md, CLAUDE.md,
CONTRIBUTING.md), follow that instead - it wins on every point.

## Labels

- One priority, `P1`..`P9`, lower number first: `P1` (wrong output, data
  loss, crash, security) > `P2` (main broken) > `P3` (wrong behaviour a user
  hits) > ... > `P7` (owed polish) > `P9` (someday). AGENTS.md "Priorities"
  has the full table.
- One or more `area:*`.
- Workflow: `claimed` (someone is on it), `blocked` (can't proceed; say on
  what), `wontfix`.

Create missing ones once: `gh label create claimed --color FBCA04
--description "Work in progress - see the last claim comment"`.

## Pick

Open, unclaimed, unblocked; lowest `P` number first, then oldest; issues a
user reported outrank owed work at the same number. An issue with no priority
is untriaged: set one by the AGENTS.md table first, never rank it last.

```bash
gh issue list --state open --limit 200 --search "-label:claimed -label:blocked -label:\"P?\"" \
  --json number,title,labels,createdAt --jq '
  map(. + {p: ([.labels[].name | select(test("^P[0-9]$"))] | first // "!")})
  | sort_by(.p, .createdAt)
  | (map(select(.p == "!")) | if length > 0 then "TRIAGE THESE FIRST (no priority; set one by the AGENTS.md table before picking):", (.[] | "  \(.number) \(.title)") else empty end),
    (.[] | select(.p != "!") | "\(.number) \(.p) \(.title)")'
```

Read the chosen issue's full comment thread before planning.

## Claim (before any code)

Label + assignee + a comment, so every other worker sees it at once:

```bash
gh issue edit N --add-label claimed --add-assignee @me
gh issue comment N --body "claimed: <who> · $(date -u +%Y-%m-%dT%H:%MZ) · branch <name> · plan: <one line>"
```

A claim is stale when its last claim comment is older than ~4 hours with no
newer comment from the claimant: comment "reclaimed from <old> (stale)" and
take it. Never work an issue with a live claim.

## Report on the issue

Findings, held patches, decisions and blockers go into comments. Blocked:
swap `claimed` for `blocked`, say on what, link with `--add-blocked-by`. New
work found along the way: a new issue (one priority, an area), referenced
from the comment.

## Close only from a pushed fix

- Fixing commit body ends `Fixes #N` (`Refs #N` for partial work).
- It closes when pushed to the default branch. Then, in the same step:
  `gh issue edit N --remove-label claimed` and comment
  `fixed in <sha> · pinned by <test names> · ships in the next release`.
- By hand only for: a duplicate (`--duplicate-of`), already done (name the
  commit), or a decided won't-do (`wontfix` label + reason). Each with a
  comment.
- At a release, comment `shipped in vX.Y.Z` on every issue it closed.

## Give up cleanly

Remove `claimed` (and your assignment), comment what was learned and where
any saved work lives, leave it open.

## Reopen, don't re-file

A user who still sees a fixed bug in a build that carries the fix: reopen the
same issue with what they saw; the claim starts again.
