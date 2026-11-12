# AGENTS.md

## What This Is

A clean-room implementation of HSMWorks: a CAM system that turns 3D models into
NC programs (G-code) for CNC milling machines. Rust, Flecs ECS, egui.

"HSMWorks" is a trademark of Autodesk. This project is independent and
unaffiliated — it reimplements user-visible behavior (operation types,
parameters, workflow) from public documentation and general machining
knowledge, nothing else.

## Autonomous Work Law

The user trusts the recommendations made in their absence and reviews them
afterwards - the work must not bother them.

- **Autonomous always.** Work the open GitHub issues (Issue Law, below) without asking for decisions. When
  a choice appears, take the recommended option and keep moving; occasional
  one-line status feedback is fine, a blocking question is not.
- **Every recommendation is recorded.** Each decision taken unattended gets
  an entry - what was chosen, why, what was considered and dropped - as the
  discussion ledger for after the roadmap is cleared. On an issue branch it
  goes in that PR's own file, `docs/decisions/<issue>-<slug>.md`, never in
  `RECOMMENDED.md` itself: every branch appending to one shared file made
  each merge re-conflict every other open PR (user, 2026-11-08: "this is
  getting annoying"). The cut gathers the files into `RECOMMENDED.md`
  (`cargo xtask decisions`, release step 2); `docs/decisions/README.md`
  has the format.
- **Never stop, never recap mid-roadmap.** There is no "wrap up and check
  in" state: the next issue is claimed when the current gate is green.
  Session summaries are for compaction, not for the user.
- **Counter-measure against stalling.** If the work risks stopping before
  the roadmap is done (idle after a gate, waiting on nothing, end-of-turn
  with open items), spawn a continuation nudge subagent to remind the
  session to claim the next issue. A stall is a bug.
- **Conflicted PRs are re-merged by their author; the maintainer
  re-checks** (user, 2026-11-10: "a skill that is intended to recheck
  current PR's with contributors name to deal with conflicts, usually
  conflicts is a remerge from main or similar easy fix. Same skill should
  also be used from a maintainers perspective to check if conflicts has
  been resolved."). Merging moves `main`, so reviewed PRs keep turning
  `conflicts` (orange). The skill `/resolve-conflicts` (`.claude/skills/
  resolve-conflicts/`) handles both sides:
  - **Contributor mode** (`/resolve-conflicts [login]`): for the author's
    open conflicting PRs, merge main into each branch (never rebase or
    force-push), keep both sides where both added lines, take main's
    snapshot PNGs and changelog files, run the targeted gate, push, and
    comment "ready for a re-check". If both sides changed the same logic,
    or a safety rule could be dropped, stop and report the hunk instead
    of guessing.
  - **Maintainer mode** (`/resolve-conflicts check`): re-read every
    `conflicts` PR. Still conflicting -> ask the author once. Clean again
    -> drop `conflicts`, then restore `merge-ready` (head unchanged), send
    it to a delta review (only a merge-of-main commit was added), or to a
    full `/review-prs` review (new code).
  Agents in a fork run contributor mode on their own PRs at the start of
  each session, after the process-file sync; the maintainer runs `check`
  after a batch of merges (e.g. after `/merge-ready`).
- **Side work rides subagents.** All testing, documentation, and knowledge
  graphing (the graphify updates) are done by subagents dispatched as
  subtasks, never inline: their completion notification is what triggers
  the progression to the next issue. This keeps the main agent
  running continuously, as "Never stop" above mandates - the main session
  plans, codes, and decides; the subagents verify, record, and graph.

## Issue Law - work is tracked as GitHub issues

Since 2026-11-07 (user: "move off the roadmap and track work as ISSUES") the
work list is the repo's open GitHub issues, read and written with `gh`.
`ROADMAP.md` is retired (2026-11-08) the way `DONE.md` was before it: both
are gone from the tree and live on in git history - `ROADMAP.md` in every
tag up to v0.14.0 (`git show v0.14.0:ROADMAP.md`), `DONE.md` up to v0.8.1
(`git show v0.8.1:DONE.md`). Read them for history and design context only;
every item they still owed is an open issue, and no work is added anywhere
but the issues. Decisions go to a per-PR file in
`docs/decisions/` (gathered into `RECOMMENDED.md` at the cut); shipped changes
go to `CHANGELOG.md`.

- **Use the gh-issues skill.** Any work that reads, files, comments on,
  labels, claims, closes or reopens an issue loads the `gh-issues` skill
  first (Skill tool), and subagents briefed on issue work are told to do
  the same. The skill carries the `gh` mechanics (list limits, search
  syntax, heredoc bodies, closing keywords); this law stays the rule
  where the two differ.
- **Pick.** `gh issue list --state open --search "-label:claimed -label:blocked -label:in-review -label:needs-research -label:\"P?\""`
  (then drop any whose comments hold a live contributor claim - "A claim
  comment is a claim", below) and take the lowest priority number first (`P1` before `P2` ... before
  `P9`, "Priorities" below), then the oldest. User-reported bugs outrank
  owed work of the same number. An issue with no priority is untriaged,
  not lowest: it is triaged ("Priorities") before any pick.
- **Priorities: `P1`..`P9`, lower is picked first** (user, 2026-11-12: "We
  might need a P1-P9 prioritization system now.. we're having issues with
  scope creep throughout the issue list"; scale chosen as "replace P0/P1/P2
  with nine levels; agents keep filing and rating their own issues", and
  read as "P4 is our normal; P1 to P3 is fix now"). `P?` (next point) is
  outside the scale.

  | Label | Means | Was |
  |---|---|---|
  | `P1` | Unsafe or wrong output: a gouge or collision that passes, wrong posted G-code, a crash, data loss | `P0` |
  | `P2` | Main is broken, or a regression in the last release with no workaround: main does not build, a gate on main is red, the app cannot open a project it saved | red-gate `P1` |
  | `P3` | Wrong behaviour a user hits, with a workaround: a legal program refused, a misleading message, a dialog that cannot do what it says | `P1` bugs |
  | `P4` | Work the maintainer asked for, for the next cut | `P1` enhancements he opened |
  | `P5` | The agent surface and the tooling keep up with what shipped: an MCP tool or skill row for a shipped command, a gate or process fix that blocks contributors | `P1` MCP / process |
  | `P6` | Accepted enhancements, not scheduled | other `P1` enhancements |
  | `P7` | Owed polish and small fixes: wording, layout, a missing test, a refusal that could say more | `P2` bugs and polish |
  | `P8` | Larger ideas that need design or research before code | `P2` enhancements, `effort:max` |
  | `P9` | Someday: not picked while anything else is free | - |

  - `P1`-`P3` are fix-now work and are for defects only: "wanted" is never
    a reason for them. Only `P1` and `P2` hold a release cut back
    (maintainer's choice, question dialog, 2026-11-12: "only P1 and P2
    block a release cut"). A minor/major cut does not start while any `P1` or `P2` issue is open; a `P3` or lower never holds one. A `P3` is worked ahead of planned
    work, but a cut does not wait for it.
  - `P4` is set only where the maintainer asked for the work in his own
    words; the issue quotes them.
  - A follow-up found while working or reviewing is `P7` unless it is a
    defect (then `P1`-`P3` by the table).
  - An agent filing an issue sets exactly one of `P1`..`P9` by this table,
    never `P?`.
  - **No priority means untriaged.** A human-filed issue without a
    priority is not the lowest: the first agent that reads it for a pick
    sets its priority by this table (with its effort label) before
    sorting, so a user-reported gouge bug is never picked last. An issue
    found with two numbers is reported to the maintainer and read by the
    lower number meanwhile.
- **`P?` is paused, not prioritised** (user, 2026-11-09: "a P? label that
  holds an issue paused until maintainer tells it to be changed"). An issue
  labelled `P?` is held by the maintainer: Pick never takes it, no lane
  slot works it, no agent claims, researches, files sub-issues for or
  re-prioritises it, and the bug/enhancement lanes skip it the same way.
  It carries `P?` *instead of* a `P1`..`P9` label, never beside one -
  an agent that finds both reports it rather than choosing. Only the
  maintainer swaps `P?` for a real priority (or closes the issue); until
  then agents may only read it, link it, and comment facts that belong on
  it (e.g. a duplicate or a related fix), without changing its labels.
  An agent filing a new issue never uses `P?` - it picks one of `P1`..`P9`.
- **Two lanes, two issues each** (user, 2026-11-07: "open up the issue
  limit to 2 bugs 2 enhancements.. as we are now just working on the latest
  issues and not the old ones"). At most four issues are worked at once: two
  in the **bug lane** and two in the **enhancement lane**. An issue labelled
  `enhancement` (and not `bug`) is in the enhancement lane; every other
  issue, unlabelled ones included, is in the bug lane. A slot is held from
  the claim until its PR is opened (the swap to `in-review`); fixing a
  review finding afterwards rides the same issue and takes no new slot, so a
  `feat` waiting for its cut never holds a lane. The bug lane picks by the order above. The enhancement lane has its
  own order, which overrides Pick's: user-filed issues (no `source:agent`)
  first, then the lower number, then the oldest, so old owed work is not
  starved by fresh bugs. When a slot frees, refill that lane at once. Each
  slot is its own worktree and branch (check free disk first: each
  worktree's `target/` is 10-20 GB). Merging follows "Done on a branch"
  by commit type, whichever lane the issue came from.
- **Start from the latest main, on a branch of its own** (user, 2026-11-07:
  "make sure we are on the latest main by pulling data from github then
  create a new branch to work off of"). Before the first edit:
  `git fetch origin && git switch main && git pull --ff-only origin main`,
  then `git switch -c issue/N-<slug>` (a worktree agent:
  `git fetch origin && git worktree add --no-track -b issue/N-<slug>
  .claude/worktrees/issue-N origin/main` - `--no-track`, so a bare pull/push
  never targets main). Worktrees live inside the repo under the gitignored
  `.claude/worktrees/`, never beside it (user, 2026-11-08: "but why are they
  outside the working folder u started up in?"), and are removed once their
  PR is pushed. One issue, one branch; every commit for it lands there, never
  on `main`.
- **A worktree agent never touches the main checkout's HEAD or tree.** It
  runs `git fetch origin` and `git worktree add ... origin/main` and nothing
  else there: no `switch`, `pull`, `merge`, `reset`, `checkout` or `stash` in
  `C:\...\Claude-001` (any checkout the maintainer works in). The main checkout
  may be running a gate (`cargo xtask gate` reads HEAD and the working tree
  before and after, and prints VOID if a pull moved them mid-run, #1201), so a
  worker's pull there voids that run for everyone. The first form above is for
  the main session only, when it is the one owning `main`.
- **A fork syncs from upstream at each cut tag, not continuously** (user,
  2026-11-09: "If we are on a forked repository (check for upstream), we
  pull from upstream, with every detected cut tag ex. jump from 0.14 to
  0.15, 0.15 to 0.16. The upstream pulls makes sure fork is working of the
  latest stable main - and not the main that is in constant change.").
  Before claiming work, check for an upstream: `git remote get-url
  upstream` (a fork of NeoTech/Camcraft has one; the canonical repo does
  not - there, "Start from the latest main" above applies unchanged). In a
  fork:
  - `git fetch upstream --tags` and find the newest cut tag,
    `git tag -l 'v*.*.0' --sort=-v:refname | head -1`.
  - If the fork's `main` does not contain it yet (`git merge-base
    --is-ancestor <tag> main` fails), sync: `git switch main && git merge
    --ff-only <tag>` (or `git merge <tag>` if the fork's main carries its own
    commits), then `git push origin main`. Sync only to a cut tag, never to
    `upstream/main`: tags are the tested, released state; upstream's main
    moves several times an hour.
  - Branch issue work from the fork's synced `main` as above. A PR into
    upstream still targets upstream's `main`; if it conflicts there, merge
    the conflicting upstream change into the branch (`git merge
    upstream/main`) on that branch only, never into the fork's `main`. The
    fork's `main` takes upstream's code only at the next cut tag; process
    files are the one exception (next point).
  - Note the tag the fork is on in the PR body ("fork synced at vX.Y.0").
  - **Process files sync on every session, not only at tags** (user,
    2026-11-10: "we need a way to inform all forks that there is changes
    to skills and agents"). The process - `AGENTS.md`, `.claude/agents/`,
    `.claude/skills/`, `xtask/` (the gate and decision tooling, #748),
    `.cargo/config.toml` (its `cargo xtask` alias), and the wrappers that
    forward to it - `scripts/gate.ps1`, `scripts/gate.sh`,
    `scripts/release.ps1`, `scripts/collect-decisions.ps1`,
    `scripts/collect-decisions.sh` - changes between releases, and a fork's
    agents must follow the current rules.
    At the start of every session in a fork, before claiming work,
    compare `git show upstream/main:.claude/PROCESS_VERSION` with the
    local `.claude/PROCESS_VERSION`. If upstream's is higher, pull just
    those files: `git checkout upstream/main -- AGENTS.md .claude/agents
    .claude/skills .claude/PROCESS_VERSION xtask .cargo/config.toml
    scripts/gate.ps1 scripts/gate.sh scripts/release.ps1
    scripts/collect-decisions.ps1 scripts/collect-decisions.sh`, commit as `chore(process): sync
    process files to upstream PROCESS_VERSION <n>` on a `process-sync`
    branch, and merge
    it into the fork's `main` (or open a PR there if the fork protects
    main), then re-read AGENTS.md before going on. A fork whose cut tag
    predates `xtask/` (the first cut after #748) also adds `"xtask"` to the
    `members` of its root `Cargo.toml` in that same commit (and lets
    `Cargo.lock` take the new package) - the wrappers call `cargo xtask`,
    which needs the member; it adds no product code. This is the one
    exception to "sync only to a cut tag": process files don't change
    product code, so the fork's code stays on the tag while its rules stay
    current. What changed and why is in the pinned issue #681 ("Process
    changes"); a fork maintainer subscribes there.
- **Process changes bump `PROCESS_VERSION` and are announced** (same
  request). Any PR that changes a process file (the list above) also
  increments the single integer in `.claude/PROCESS_VERSION`, and when it
  merges, the merging session adds a comment to the pinned issue #681:
  `PROCESS_VERSION <n>` · PR #M · what changed · what a fork must do. The
  reviewer treats a missing bump as a requested change. Two process PRs
  open at once both bump to the same number; whichever merges second
  takes the next number when it merges main. At a cut, the release notes
  get a short "Process changes" section listing the versions since the
  previous tag.
- **Overlapping issues stack.** Open PRs wait unmerged (a `feat` until its
  cut, a fix until it is reviewed), so an
  issue touching code an open PR changes branches off that PR's branch
  (not main) and its PR says "stacked on #M". Before the parent merges (any
  time while it is unmerged), retarget the child FIRST (`gh pr edit <child>
  --base main`); then merge the parent; then the child takes `git merge
  origin/main`, is re-gated and merges. Never the other way round: `gh pr
  merge --delete-branch` on the parent deletes the child's base branch and
  GitHub closes the child PR; recovering it means restoring the deleted
  branch, reopening and retargeting (0.12.1 lost #82 that way and replaced it
  with #86). An overlap that can't stack waits for the parent PR to merge.
- **Claim before any code.** One command, so every agent sees it at once:
  `gh issue edit N --add-label claimed --add-assignee @me` plus a comment
  `gh issue comment N --body "claimed: <session or agent name> · <UTC time> ·
  worktree/branch <name> · plan: <one line>"`. Never work an issue that
  carries `claimed`, with two exceptions. One: its last claim comment is
  one of the owner account's own agents' and older than 4 hours with no
  newer comment from the claimant - then it is stale: comment "reclaimed
  from <old claimant> (stale)" and take it. Two: the claim behind the
  label is a contributor's and it is withdrawn, released or stale by the
  rule "A claim comment is a claim", below - then release it as that rule
  says, and the issue is free. Never on a `P?` issue: no agent relabels or
  works one.
- **Agent-opened issues say so** (user, 2026-11-07: "All Issues opened by
  an Agent should have the tag source:agent"). Every issue an agent opens
  carries the `source:agent` label alongside its `P*`, `effort:*` and `area:*` labels -
  including a bug the user reported in chat and an agent filed for them.
  An issue a human opens on GitHub never gets it. Agents and the user share
  one GitHub account, so the label, not the author, tells them apart.
- **Every issue is weighted by effort** (user, 2026-11-10: "we need a way
  of 'weighting' an issue ... to manage token spend on different levels of
  complexity of tickets"). Beside its `P*`, every issue carries exactly one
  effort label: how hard the *work* is, not which model does it. `P*`
  still decides which issue is worked next; `effort:*` decides who works
  it.

  | Label | The work | Examples |
  |---|---|---|
  | `effort:low` | mechanical, one file, no new logic | docs, labels, a test fixture or field fix (#711), a string or layout tweak |
  | `effort:medium` | one module or crate, a known pattern | a UI page change, an MCP tool over an existing command, a new machine file |
  | `effort:high` | several crates, new behaviour with its tests | a new undoable command with its MCP tool, simulation or post logic, a refusal law |
  | `effort:max` | new algorithms, formats or open design | geometry or toolpath algorithms, collision laws, a file format (DXF, #743), research-shaped work |

  - **Set at filing.** Whoever files an issue sets its effort label with
    its `P*` and `area:*`, from the table. A human-filed issue without
    one gets it at triage (the first agent that reads it for a pick).
    The `classify-effort` skill (`.claude/skills/classify-effort/`) is how
    the label is chosen: `/classify-effort <N>` for one issue,
    `/classify-effort` for every open issue without one,
    `/classify-effort audit` to list labels that look wrong (read-only).
    It only sets a missing label; it never lowers or replaces one.
  - **Safety floor.** Anything that can change posted G-code, a collision
    or gouge check, stock simulation or geometry is at least
    `effort:high`, however small the diff; every `P1` is at least
    `effort:high`. The Prime Directive is never handed to the cheapest
    tier.
  - **Who works it.** The main session picks by Pick and the lanes as
    before, then dispatches the issue to the worker agent of its effort -
    `.claude/agents/issue-worker-<effort>.md`, each running the shared
    `/work-issue` skill (claim, worktree, fix, pinning test, quick gate,
    draft PR). The worker never merges; the main session keeps the
    review and merge decisions. The model behind each effort is set only
    in those agent files (`model:`), so a fork on other models changes
    four lines, not this law. If an agent's model is unavailable (a 404),
    run it without the `model:` line (the session's model) and note it in
    the claim comment.
  - **Review tier.** The review of a worker's PR runs at least on the
    `effort:medium` model, and at the `effort:high` model for anything
    under the safety floor. A cheap review of cheap work is how a bug
    gets through.
  - **Escalation, never silent.** A worker that fails its quick gate
    twice, or finds the work is bigger than its label, stops: it comments
    what it learned and where any held work lives, raises the label one
    step (`low` -> `medium` -> `high` -> `max`), and releases the claim; the
    next pick goes to the higher tier. At most one escalation per run.
    Only the maintainer lowers an effort label.
  - **Measured.** The PR's `review-cost` comment names the effort and
    whether it escalated, so the table can be recalibrated from data
    (escalation rate and cost per tier), not by feel.
- **A claim comment is a claim** (user, 2026-11-08: "a comment should
  suffice ... if we come across a claimed issue thru comment we should
  just label it as such - and vice versa if they remove their comment,
  remove their claim label for em as part of maintenance.. Otherwise the
  comment stands as claimed." - that last sentence is superseded since
  2026-11-11: a claim stands only while it is live, below). Only
  collaborators can set labels, so
  anyone else claims an issue by commenting on it with a line starting
  `claimed:` (or `/claim`). The comment is the claim; the label only
  mirrors it.
  - **Check before claiming.** Before `Claim before any code`, read the
    issue's comments (`gh issue view N --comments`). A claim comment by
    anyone other than the owner account (agents post as the owner) means
    the issue is taken: do not work it, even if it has no `claimed` label -
    unless the claim is withdrawn, released or stale (below).
  - **Mirror it.** An issue with a live claim comment (next point) but no
    label gets `claimed` and the claimant as assignee (if GitHub refuses
    the assignee, the label alone), plus one reply: "Claim recorded for
    @<login> - label and assignee set (only collaborators can set labels;
    your comment is the claim)." Only a LIVE earlier claim by someone else
    holds an issue against a new claimant; a stale or ended one does not.
    When a new claimant comes over a stale earlier claim, release the
    stale one first (its release comment ends it, next point), then mirror
    the new claim: the label and the assignee always follow the newest
    live claim.
  - **A claim must be live - an old one cannot stall an issue** (user,
    2026-11-11, on eight issues a contributor's agents had claimed days
    earlier and left: "unclaim those 8 old ones and let someone pick em up
    and fix em they can't stall"). A contributor's claim is live while at
    least one of these holds: an open PR by the claimant for the issue; a
    commit on the claimant's branch for it in the last 48 hours; or a
    comment by the claimant on the issue in the last 48 hours (the claim
    itself, progress, findings - any comment of theirs counts; the rule is
    kept simple on purpose). Research claims have no branch, so the
    comment is what keeps them live. With none of the three, the claim is
    **stale**: do not mirror it; if it carries `claimed`, remove the label
    and the assignment and comment "claim by @<login> released (stale: no
    open PR, branch commit or comment from the claimant for 48 h) - issue
    is free again; claim it again with a new comment if you are working on
    it". That comment ENDS the claim (next point): a later comment by the
    claimant does not revive it, only a new claim line does. Ended beats
    live: a comment that withdraws or releases a claim is not activity
    that keeps it live, and an ended claim is never live again. The 4 h rule
    above stays the rule for the owner account's own agents. A `P?` issue
    is never relabelled by an agent, stale claim or not: report it to the
    maintainer instead.
  - **Withdrawn or released.** A claim also ends when its comment is
    deleted; when the claimant's later comment withdraws it (`unclaim`,
    `/unclaim`, "claim withdrawn", "releasing this claim", "the issue is
    free" - read the claimant's LAST comment, not only the claim line);
    when the maintainer releases it (a comment saying the claim is
    released by the maintainer); or when an agent has released it as stale
    (the comment above). Then: remove `claimed` and their assignment, and
    comment "claim by @<login> withdrawn - issue is free again" (not needed
    where a release comment already says so). Only a NEW `claimed:` /
    `/claim` comment posted after that is a claim again; an ended claim is
    never mirrored back from its old comment.
  - **Maintenance.** Every `/review-prs` run (step 4) and every issue pick
    does this sweep for the issues it touches: label the live
    comment-only claims, and drop the label where the claim behind it is
    withdrawn, released or stale. A `claimed` label backed by one of the
    owner account's own agent claims follows the 4 h rule above instead.
- **`needs-research` is research, not code** (user, 2026-11-08: "the new
  label needs-research ... covers larger open questioned issues that needs
  to be broken down into issues and closed, before work can be picked up").
  An issue labelled `needs-research` is a large idea with open questions
  (e.g. #290 Wire/Sinker EDM, #291 documentation site). Pick never takes it
  for code. Researching it is claimed like any issue ("Claim before any
  code", with `plan: research`), but takes no branch, no PR and no lane
  slot. Working it means: answer the open questions in issue comments
  (research, recorded decisions), then file the concrete sub-issues it
  lists - each with one `P*`, one `effort:*`, `area:*`, `source:agent` when an agent files
  it, a "Done means" line and "Part of #N" - and link them back from the
  umbrella. When every sub-issue is filed, drop `needs-research` and close
  the umbrella with a comment naming them - it is not kept open, so Pick
  can never take it for code. Only the sub-issues are picked for code.
- **Report on the issue, not in prose files.** A finding, a held patch, a
  blocker or a decision taken goes into an issue comment. If the work is
  blocked, swap `claimed` for `blocked` and say on what. If a fix shows new
  work, open a new issue for it (labels: one `P*`, one `effort:*`, one or more `area:*`)
  rather than burying it (and `source:agent`).
- **Done on a branch = pushed + PR, never merged to main** (user,
  2026-11-07: "when closing an issue we should push the branch, not merge
  to main"). A `fix` / `perf` / `refactor` PR merges once its independent
  review is clean and its quick gate green, with no cut (patch cuts are
  retired - "Commits and Releases"); a `feat` or breaking PR merges at the
  minor/major cut; docs/process/CI-only PRs merge once reviewed. The fixing commit ends its body with `Fixes #N` (`Refs #N` for
  partial work, which never closes). When its gate is green:
  `git push -u origin issue/N-<slug>`, then
  `gh pr create --draft --base main --head issue/N-<slug> --title
  "<conventional subject>" --body-file <file>` - the body says what changed, the readings,
  the pinning test(s), a `Changelog:` line (the one-line row text, or
  `none` for docs/test-only) and ends `Fixes #N`. Then, in the same step:
  swap `claimed` for `in-review`, and comment `fix on issue/N-<slug> · PR #M
  · pinned by <test name(s)> · merges when reviewed` (a `feat`: `merges at
  the next cut`). The issue closes
  when the PR merges to `main` (GitHub closes it from the keyword). A
  review finding is fixed on the same branch and pushed (plain push, never
  force); whoever fixes it re-adds `claimed` with a claim comment while
  working and swaps back to `in-review` when pushed. A PR closed unmerged
  (dropped, or held by the user with a PR comment) swaps `in-review` for
  `blocked` (held, say on what) or for nothing (dropped, say why), with an
  issue comment - an issue never sits in `in-review` without an open PR.
- **Every PR opens as a draft** (user, 2026-11-08: "All PR's should be done
  as drafts to start with. And not flagged ready to review until either
  user saids its ok or the tests are done. Once its been flagged ready to
  review no more pushes should be performed"). A PR is created with
  `--draft` and stays a draft while work, gating and review fixes land on
  it. It is flagged ready (`gh pr ready <M>`) when the user says so, or
  when a **quick pass** confirms its gates hold (user, 2026-11-08: "the
  quickpass was to make em ready for review.. so flag em"): it merges
  cleanly with `main`, its author's quick gate is green on its final head
  with the pinning tests named, its body is complete (what changed, how
  tested, what was not exercised, `Changelog:`, `Fixes`/`Refs`), and it
  keeps this file's laws. The quick pass reads the PR, it does not
  rebuild; the full independent review comes after, on the ready PR
  (`/review-prs`, "Outside PRs are reviewed" below). Once ready, its branch takes **no more pushes**: a late
  finding, a needed `git merge origin/main` or any other change first turns
  it back into a draft (`gh pr ready <M> --undo`), says why in a PR
  comment, is pushed and re-gated, and only then is flagged ready again.
  Merging (by the main session for fix/docs PRs, at the cut for `feat`)
  takes only a ready PR, never a draft.
- **Never close by hand without a commit**, except: a duplicate (link the
  original), already done (name the commit that did it), or a decided
  won't-do (label `wontfix`, give the reason). Each needs that comment.
- **User-reported bugs reopen, never re-file.** If the user still sees the
  bug in a build that carries the fix, reopen the same issue with what they
  saw; the claim starts again.
- **When a PR merges**, drop `in-review` from its issue (GitHub closes it
  from `Fixes #N`). **At a release cut**, comment `shipped in vX.Y.Z` on
  every issue closed since the previous tag - the cut's own merged PRs plus
  the fix PRs merged since - and drop any `in-review` left over.
- **Giving up without a fix:** remove `claimed`, comment what was learned,
  leave it open. A pushed branch with held work is named in that comment;
  an empty one is deleted (`git push origin --delete issue/N-<slug>`).
- **Changelog.** Every PR that changes behaviour carries its row text in
  its body (`Changelog:` line); the row is written into `CHANGELOG.md` on
  the release branch at the cut, for every PR merged since the previous tag
  (fix PRs merge between cuts), not on the issue branch (each branch
  writing the table's top row would make every open PR conflict):
  `| <short merge commit> | <YYYY-MM-DD> | <change, one line> (#N, PR #M) |
  X.Y.Z |`. Docs-only and release-mechanics PRs need no row. Rows marked
  `Unreleased` (from before the branch flow) take the version at the next
  cut.

## Clean-Room Protocol

- No reading, decompiling, disassembling, or tracing HSMWorks binaries or
  source. No code derived from their code.
- No copying their text: UI strings, docs, help content, parameter names where
  original, comments, or bundled post processors.
- No assets of any kind from their installer or install tree — icons, images,
  tool/NC libraries are all made from scratch.
- Reference material is limited to: Autodesk's published user documentation
  (behavior only, not implementation), public G-code standards (RS-274, ISO
  6983), machine-tool handbooks, and our own design notes.
- New geometry or toolpath algorithms must be written from published technical
  literature, not from inspection of HSMWorks output forensics.
- Every module carries a one-line provenance comment: what it implements and
  from which public source it was specced.

## Prime Directive: Never Crash, Never Gouge

A CAM program drives a spindle in front of a human. The output is either
correct, or visibly refused with a diagnostic. Silent wrong answers — a crash
or a toolpath that gouges, collides, or cuts where it shouldn't — are the
worst failure mode, without exception.

- Handle errors or propagate them - do not use unwrap() or expect().
- Degenerate input (zero-area triangles, self-intersecting shells, nonsense
  tolerances, corrupt STL/STEP models) → clean error, not a crash or hang.
- Anything a collision check or simulation flags as unsafe must block the
  post, never pass through silently.
- No panics in systems or on any user-triggered path. `unwrap()`, `expect()`,
  and `panic!()` are forbidden outside tests and `#[cold]` unreachable arms.
  Prefer `Option`/`Result` and graceful fallbacks. A crash is a bug, always.
  No exceptions.
- Use safe numeric types for geometry: saturating, checked, or clamped ops -
  no unchecked floats in bounds/index math.
- Validate early: reject malformed setups and parameters at command
  creation, not deep inside a generator where the failure is opaque.
- Prefer recoverable states: if an operation can only be partly computed,
  mark it "incomplete" rather than emit a subtly wrong toolpath.
- Log diagnostics, never silence.
- If a leak is found, fix it now, not later. Treat leaks as critical bugs:
  they crash the app eventually, and "we'll clean it up later" always makes
  it worse.

## Everything Is a Command

UI state changes are not ad-hoc field pokes - they are commands with entity
references. Undo (Ctrl+Z) and redo (Ctrl+Y) iterate the command history and
invert: the history is the single source of truth for what undo/redo can do.
Any UI-driven change goes through it, including parameter edits in the
operation property panel.

- `CreateOperation` / `DeleteOperation` → operation entities
- `ChangeTool` / `EditOperationParams` → operations and setups that had a
  tool, stock, or depth changed (write the new value on command creation,
  not via systems; undo must restore the previous value)
- `SelectEntity` → selection (which operations/models are highlighted)
- `ActivateSetup` → the active setup
- Ephemeral UI state (hover, drag previews, simulation playback position) is
  immediate-mode only - not undoable

Systems and UI never write these fields ad hoc; they only respond to commands.
If a UI change has no command for it, that's a bug - add one.

**Commands ship with an MCP tool** (#128, #356). The MCP server lets an AI agent
drive CamCraft headlessly. A PR that adds a user-facing command also adds its
MCP tool (and script verb where it fits) in `crates/app/src/mcp/`, or files a
`source:agent` follow-up issue `MCP: <command>` and names it in the PR body. Every
MCP issue (an `MCP:` title, or work on `crates/app/src/mcp/` or the camcraft-mcp
skill) carries `area:mcp`. Owed agent-surface work (a tool, script verb or
skill row for a shipped command) is `P5` (user, 2026-11-10: "find all MCP
tickets and make P1" - the top level then, `P5` since the scale became
P1-P9 on 2026-11-12); a defect in MCP code is rated by severity like any
defect, `P1`-`P3`. A paused one stays `P?`. This keeps the agent surface up with the GUI. MCP
tools call the same undoable commands the GUI uses, so undo/redo history and
invariants hold headlessly.

Components are auto-registered: declare them in `crates/app/src/schema.rs`
and the startup hook (`flecs_bits::register_schema`) handles the rest, in every
binary. Current policy: components are Rust-owned types (`Vec`/`Box` fields),
freed by the delete hooks flecs_ecs registers for them. The invariant that
matters is unchanged: deleting an entity must free ALL associated resources
(e.g. the cached toolpath and rest-material meshes of a deleted operation).
Per-type arenas for C-style `raw_data` storage are a deferred performance
optimization (M5.3-era), not a current requirement; if a type ever adopts
`raw_data`, it must use a per-type arena - never a global allocator - and
must carry a delete hook that frees everything it owns.

## Assets

`assets/ui/` holds PNGs for toolbar and tree icons; `assets/ui/fonts/` holds
the fonts. The UI must render them — loading the files alone isn't enough;
wired-in usage in each panel's draw code is required.

Icon sources: free, attribution-safe sets only - MIT/ISC/Apache-2.0/BSL
licensed libraries may be bundled without crediting end users; primary pick is
[Tabler Icons](https://tabler-icons.io) (MIT). Never icons8's free tier
(requires attribution), never paid-without-need. 32x32 PNGs are fine; render
at 16px. Keep the copyright/license texts of any bundled set in
`assets/ui/LICENSES.md` (this is also the home for third-party notices like
clipper2-rust's BSL text) - retention is a license condition, not a credit.
Operation-specific glyphs are drawn original when no generic set fits. All
icons referenced from `crates/ui/src/egui_app/icons.rs`. Icons and post-processor
templates (see below) must be original or licensed without attribution —
never adapted from HSMWorks material.

## Dependencies

Layering is strict: ui → app → flecs. UI panels, widgets, and dialogs must
not use sys_* or world-level direct calls; put those in app. Only app-layer systems and commands use raw Flecs calls.

Schema components live in the schema module (`crates/app/src/schema.rs`):
the `#[derive(Component)]` structs and their CPOD decls. Other app modules
must not declare components.
Geometry parts are components (`app::model::Part`); use `model::child_parts` /
`model::child_parts_of` to traverse - in UI and systems. Components don't
reference each other. `Part` stores `child: Entity` (not parent); systems
query directly (no helper accessors). `decompose` stores facets directly into
`Part`.

Check only touched files: `-p` per touched crate - `camcraft-ui`,
`camcraft-app`, `camcraft-motion`, `camcraft-sim`, `camcraft-post`,
`camcraft-tools`, `camcraft-cl`, `camcraft-geometry` (not the whole
workspace, which rebuilds flecs). The M4.1 split landed: geometry/cl/tools/
post/sim/motion are their own crates; app's `operation` and `project` modules
are split along section seams.

## Code shape (Phase 4 onward)

Phase 4 refactors the codebase for reuse; work in the shape it is heading to:

- **400-line files.** No source file grows past ~400 LOC; split along the
  domain seam instead of stacking another section onto a large file.
  **Exempt: post-processor scripts** (`assets/posts/*.cps`; user, 2026-11-08:
  "CPS files cannot follow the 400 line rules as its a script to be
  executed by a post processor, and it needs what it needs"). A post script
  is one file the post engine loads and runs for one machine dialect, so it
  stays whole at whatever length its dialect needs; review it for
  correctness, not length.
- **One concern per subcrate.** Motion types, operations, tool management,
  heights/offsets, movement and simulation become isolated subcrates (M4.1).
  Until the split lands, keep those boundaries honest inside `app`: new code
  goes in the module that will become its crate's seed, and cross-domain
  calls go through the seams that will become crate APIs - not through
  private helpers of another domain.
- **Stays buildable everywhere.** No platform-specific assumptions in core
  code (M4.2 ships Windows/Linux/macOS builds from the same source).
- **Neutral naming.** M4.3 renamed the product label hsmworks to camcraft
  everywhere, crates included; nothing - string, comment, or identifier -
  carries the old brand. Legitimate trademark mentions ("no HSMWorks
  material consulted", "a clean-room implementation of HSMWorks") name
  Autodesk's product and stay: they're true statements, not branding.

## Testing

`crates/app/tests/` (app + commands) and `crates/ui/tests/` (UI/event tests
which spawn the real app via `test_harness.rs`; helpers in `ui::test_harness`,
app-level fixtures in `crates/app/tests/common/mod.rs`). Each future subcrate
carries its own tests inside its crate (M4.1).

**Three gate tiers - never hours per change** (user, 2026-11-07: "break up
those test suites so we don't have to run hours on hours of tests"). One
command runs them on Windows, macOS and Linux alike (#748): `cargo xtask
gate <tier>`, from the workspace's `xtask/` crate. `scripts/gate.ps1`
(`-Tier quick`, `-Test <name>`, ...) and `scripts/gate.sh` (`quick`,
`test <name>`, ...) still work: they are thin wrappers that forward to it.

1. **quick** (per patch, by the agent or gate-runner, target ≤ 5 min): only
   the test binaries the diff touches, picked from the gate maps and the
   touched crates (`cargo xtask gate quick [--base main]`); debug; no heavy
   lanes.
2. **batch** (main session, before committing a batch, target ≤ 20 min):
   every debug suite except those tagged slow, the ui suites from
   `crates/ui`, clippy (`cargo xtask gate batch`). No release-mode heavy
   lanes.
3. **release** (only at a cut, run locally): everything - all debug
   suites, the `--ignored` heavy lanes in release (`cargo xtask gate
   release`).

Any tier takes `--dry-run` to print its plan without running it; logs go
to `gate-logs/<tier>-<timestamp>/`.

**Shared compile cache (optional, #900).** Every worktree has its own
`target/`, so a fresh one recompiles every third-party crate. With
[sccache](https://github.com/mozilla/sccache) (`cargo install sccache
--locked`, user account only) those compilations are cache hits across
worktrees. `cargo xtask gate` sets `RUSTC_WRAPPER=sccache` and
`SCCACHE_CACHE_SIZE=20G` for its cargo calls when `sccache` is on PATH (an
existing `RUSTC_WRAPPER`, or `CAMCRAFT_NO_SCCACHE=1`, wins); a machine without
it builds as before, and release packaging never uses it. For plain `cargo`
set `RUSTC_WRAPPER=sccache` only when `sccache` is on PATH (cargo fails if the wrapper is missing), and never export `CARGO_TARGET_DIR`; `cargo xtask release` clears any wrapper for the packaging build. The cache lives in the sccache
default dir (Windows `%LOCALAPPDATA%\Mozilla\sccache\cache`, Linux
`~/.cache/sccache`; `SCCACHE_DIR` moves it). Hits need each worktree's default
`target/`: sccache hashes `CARGO_*` env vars, so a `CARGO_TARGET_DIR` env var
makes every worktree miss. Dev builds carry line tables only (no variable
debug info; `--config profile.dev.debug=true` restores it for a debugger).

**GitHub Actions minutes are budgeted** (3000/month; Windows bills 2x,
macOS 10x). Nothing runs on push or PR: every gate tier runs locally
through `cargo xtask gate`. GitHub runs only `release.yml` on a pushed tag
(the build that publishes the artifact). `ci.yml` and `nightly.yml` are
manual (`workflow_dispatch`) for the rare case a clean-machine run is
wanted. Never add a `push`, `pull_request` or `schedule` trigger without
the user's say-so.

**Test the change, not everything.** A fix is proved by the test(s) that pin
it, run one by one: `cargo xtask gate test <name> [--crate c] [--binary b]
[--exact]` (or plain `cargo test -p <crate> --test <binary> -- <name>
--exact`). A whole suite runs only when the
change touches what that suite covers; the full set runs only at a release
cut (user, 2026-11-07: "tests should also be possible to run one by one …
so we are testing changes rather than everything every time").

Batch skips the debug binaries in `SLOW_BINARIES` (`xtask/src/plan.rs`,
pinned by its unit tests) (motion finish3d, pivot5d, flow; app
center_cut_multiaxis - each ~40-50 s); release runs them. camcraft-kernel
runs in release, and in quick only when kernel itself is touched. Quick
prints what it chose before running: the changed `#[test]` fns of a changed
suite (`--exact`), and the gate-mapped suites of a changed src module; a
change to shared test code (tests/common, goldens, snapshots) runs that
crate's suites; geometry/cl/Cargo changes fall back to batch; a change
under `xtask/` runs xtask's own unit tests.

A test that takes more than ~60 s in debug moves to the heavy lane (with
its lane named in the ignore reason) or gets a slimmer fixture; the batch
tier stays fast by law, not by hope.

Gate yourself narrowly, not by re-running everything: build once with
`cargo test -p camcraft-app -p camcraft-ui --no-run`, then run the individual
test binaries the diff touches (`target\debug\deps\<name>-<hash>.exe`, in
parallel - they take no cargo lock). Each suite's header carries the module
map (`Gate map (M4.4): covers app::x, app::y...`) - pick binaries from it,
not from the whole tree. The debug ladder's budget is under ~5 min wall;
everything slower is in the heavy lane below, not in yours. Under "Side
work rides subagents" this ladder is run BY a gate subagent: it reports
pass/fail per binary with failure output, and its report is what lets the
main session commit. Never hand the subagent both the code change and its
verification - the main session owns the code and the commit decision.

Laws and verdicts run in different lanes (M4.4): tests that MEASURE geometry
clean (envelope scans, full-precision projections) or generate over the big
fixtures are `#[ignore]`d - `cargo test --release -p camcraft-app --
--ignored` executes them in minutes and belongs at milestone close-out and
pre-push, never per-edit. The ignore reason names its lane. Coverage must
not ride the split away: `#[test]` count before and after must match, and
the ignored set keeps its golden asserts. UI snapshot binaries resolve their
goldens from `crates/ui` - run them with that CWD.

**Pixel snapshots are release-lane** (#255). The UI's PNG goldens
(`crates/ui/tests/snapshots/*.png`) are `#[ignore]`d out of quick and
batch and run only in the release tier's ui snapshot lane (debug,
`--ignored snapshot`, CWD `crates/ui`). A PR never changes a snapshot
PNG - not to "refresh" a conflict, not to accept a new render - and a
`UPDATE_SNAPSHOTS=1` refresh on merge is no longer a habit: the goldens
are regenerated once at the minor/major cut, in one commit on the release
branch, after a human look at the `.diff.png`. What a snapshot guards is
asserted in the default suite by its structural twin
(`crates/ui/src/egui_app/tests/structural.rs`): AccessKit labels, roles,
enabled/disabled state and window presence, plus the painted strips' text
and coarse placement - text-only, so it holds on every OS and merges like
code. A UI change updates those assertions, not the PNGs.

`VisualRunner` is auto-enabled: windows show, run fast, and stay open if closed
early; hidden runs cap at 30s; auto-close after 10s when no test is pending; on
crash or timeout, the window is left open.

Add headless tests for toolpath generation (golden CLFile/G-code outputs
committed under `tests/golden/`), stock simulation, and post processing; fuzz
tests for the geometry importers and the toolpath generators (degenerate input
must error, never crash or hang).

## Knowledge Graph

`graphify-out/` holds a graphify knowledge graph of this repo: code extracted
structurally (AST), docs and images semantically. Generated output - never
hand-edit it, never commit it.

Query it before falling back to grep for structural questions:

- `graphify query "<question>"` - how does X work, what calls Y, trace a flow
- `graphify path "A" "B"` - shortest connection between two concepts
- `graphify explain "X"` - plain-language summary of a node and its neighbors
- `graphify affected "X" --relation calls` - who breaks when X changes
  (the graph is strongest here: it knows the test-to-command wiring)

Keep it current as we work:

- Code-only changes → `graphify update` (re-extracts changed files via AST,
  free and no LLM; run it as part of wrapping up a task)
- AGENTS.md / icon changes → `/graphify . --update` instead, so
  the semantic side is re-extracted too
- After renames or refactors that move many files → full `/graphify .` rebuild;
  the shrink guard refuses a smaller graph unless `--force` is passed
- Expected warning: `tests/golden/*.cl` read as Common Lisp and contribute
  nothing to the graph (our CLFile dumps, not source) - ignore it

Trust calibration: every edge carries EXTRACTED (1.0, read from source),
INFERRED (0.55-0.95, model-reasoned), or AMBIGUOUS (0.1-0.3) - verify an
INFERRED edge against the code before acting on it, never act on AMBIGUOUS.

## Commits and Releases

[Reference](https://github.com/qoomon/conventional-commits)

Format: `<type>(<optional scope>): <subject>` - e.g. `fix:` or
`feat:`.

Types in use: `feat` (new capability), `fix` (wrong behaviour corrected),
`perf`, `refactor`, `test`, `docs`, `build`, `ci`, `chore`. A breaking change
says so: `feat!:` / `fix!:` or a `BREAKING CHANGE:` footer (an incompatible
project-file or post-output change, a removed parameter or command). The
fixing commit's body ends with `Fixes #N` (Issue Law).

**Versioning is semantic: MAJOR.MINOR.PATCH, derived from the commits.**
A cut takes the highest bump among the commits since the last tag:

| Commits since the last tag | Bump | Example |
|---|---|---|
| a breaking change (`!` or `BREAKING CHANGE:`) | MAJOR | 1.4.2 → 2.0.0 |
| any `feat` | MINOR | 0.8.2 → 0.9.0 |
| only `fix` / `perf` / `refactor` (behaviour-neutral or corrective) | no cut - ships in the next minor | - |
| only `docs` / `test` / `ci` / `build` / `chore` | no release | - |

While the major is 0 (until the clean-room audit releases 1.0.0), a breaking
change bumps MINOR instead and its release notes say "breaking" first.

**When to cut: only MAJOR and MINOR.** Patch cuts are retired (user,
2026-11-07: "No patch cuts at all is the way to go, do not spend time or
tokens on those. Make sure they are tested but do that as a subtask that we
are not waiting for"). A fix merges to `main` once reviewed and gated and
ships in the next minor or major release; no PATCH version, tag or zip is
made. Landing a feature (or a breaking change) ends in a minor (or major)
cut - cut as soon as the `feat` PRs are gated and waiting, not on a
calendar. `release.yml` builds and publishes on `vX.Y.0` tags only (~30-40
billed Actions minutes each); PRs run no Actions (no `pull_request`
trigger), so the PR flow costs no minutes.

**Fixes are tested in the background.** After a fix merges, the main
session dispatches a gate-runner subagent to run the batch tier on
`origin/main` in the MAIN checkout, reusing its `target/` - never a fresh
worktree, whose own ~25 GB `target/` once filled the disk - and does not
wait for it: it claims the next issue at once. The main checkout is shared,
so the gate runs there only when it is on a clean `main`
(`git pull --ff-only` first) with no `release/*` branch checked out, no cut
in progress and no other gate running; otherwise it waits, and the main
session starts no cut until a running gate has finished. The main session
therefore does its own issue work in a worktree while a gate runs. Before
dispatching, check the free disk space; `cargo clean` the main `target/`
only when no gate or cut is running and it is past ~60 GB. A red result is a new issue (`P1` if it is wrong output or a
gouge, else `P2`, `source:agent`) naming the failing tests and the merge
that broke them. A red result does not revert the merge by itself; the
issue is worked in the normal order, and a minor/major cut does not start
while any `P1` or `P2` issue is open (a `P3` or lower never holds one). The full release tier runs only at a
minor/major cut. A fix-only stretch produces no build; to hand-test one, run
`cargo xtask release --version <next> --no-tag --skip-gate` (or the wrapper
`scripts/release.ps1 -Version <next> -NoTag -SkipGate`) locally on a scratch
branch off main (it refuses `--no-tag` on main) and discard the branch and
its bump (no tag, no push).

**A cut goes PR review → merge to main → tag** (user, 2026-11-07: "when
cutting a tag, we are doing that by PR review, merging to main, then we cut
a tag"):

1. **Review and merge the issue PRs.** Each open `in-review` `feat` or
   breaking PR gets a
   review by a subagent that did not write it (`/code-review` on the PR, or
   a reviewer briefed with the diff, not the author's conclusion); findings
   are fixed on that branch and pushed. A PR that no longer merges cleanly
   takes `git merge origin/main` on its branch (an older branch that still
   appends to `RECOMMENDED.md` keeps both sides), is re-gated (quick), and
   pushed. Then
   `gh pr merge M --merge --delete-branch` - merge commits, never squash or
   rebase, so branch commits keep the hashes the issue comments name. A
   worktree still holding the branch is removed first (the local delete
   fails otherwise). From step 1 to the tag, nothing else merges to main. Stacked PRs: retarget each child to `main`
   before merging its parent (Issue Law, "Overlapping issues stack"), then
   merge parent-first.
2. **Release branch.** Sync main (`git pull --ff-only`), then
   `git switch -c release/vX.Y.Z`. The version is the highest bump among
   the commits since the last tag (table above). Run
   `cargo xtask decisions --version X.Y.Z` (appends every
   `docs/decisions/*.md` to `RECOMMENDED.md` under a `vX.Y.Z` heading and
   removes them; it goes in the same docs commit). Write `RELEASE_NOTES.md`
   and the `CHANGELOG.md` rows for every PR merged since that tag (one per
   behaviour PR, from its `Changelog:` line, plus any `Unreleased` rows).
   The release notes also get a short "Process changes" section listing
   each `PROCESS_VERSION` since the previous tag (taken from the pinned
   issue #681). Commit all of it as `docs: X.Y.Z release notes;
   changelog`; then `cargo xtask release --version X.Y.Z --no-tag
   --installer` (Windows; `scripts/release.ps1 -Version X.Y.Z -NoTag
   -Installer` is the same command through its wrapper; on macOS and Linux
   it hands packaging to `scripts/package-*.sh` and refuses `--installer`)
   - it runs the release gate tier on that tree (pass `--skip-gate` only if
   this exact tree already passed it), commits the version bump, builds
   the zip and installer, and makes no tag (it refuses `--no-tag` on main).
3. **Release PR.** `git push -u origin release/vX.Y.Z`,
   `gh pr create --base main --title "chore(release): X.Y.Z"`, reviewed like
   any PR. Record the branch tip first (`tip=$(git rev-parse
   release/vX.Y.Z)` - `--delete-branch` removes the name), then
   `gh pr merge --merge --delete-branch`.
4. **Tag the release merge commit.** Sync main; `m=$(git rev-parse main)`
   is the release PR's merge commit (nothing else merged since step 1).
   `git diff $tip $m` must be empty - the zip's tree is the tagged tree. Then
   `git tag -a vX.Y.Z -m "CamCraft X.Y.Z" $m` and `git push origin vX.Y.Z`.
   If the diff is not empty, do not tag: something reached main mid-cut;
   start the cut again from step 2 on a fresh release branch. Comment
   `shipped in vX.Y.Z` on every issue the cut closed.

**`main` is protected on GitHub** (user, 2026-11-07: "so we don't get
unauthorized pushes into main instead they follow the rules"): every change
reaches it through a PR (0 approvals required - the agent and the user push
as the same account, which cannot approve its own PR; the review is the
subagent's, for our own PRs; outside PRs wait for the user, below), admins included; no force-push, no deletion; open review
conversations must be resolved before a merge; the repo allows merge
commits only (squash and rebase merging are off). A direct `git push origin
main` is refused by GitHub - the legacy direct cut is gone. Tags are not
covered and are pushed as before (step 4).

**Outside PRs are reviewed with `/review-prs`** (user, 2026-11-07: "Add the
use of the /review-prs into agents.md"). A pull request from a fork gets a
review by the `PR-reviewer` agent (`.claude/agents/PR-reviewer.md`),
dispatched by the project skill `/review-prs` (`.claude/skills/review-prs/`) -
run on demand (`/review-prs`, or `/review-prs <N>` for one PR) and on a loop
(`/loop 10m /review-prs`, every 10 minutes). The agent trial-merges the PR with current main in
its own worktree, builds, runs the suites it touches and clippy, checks this
file's laws, and posts a plain review **comment** whose first line is
`<!-- pr-reviewer: <head sha> -->` (the marker that stops a second review of
the same push), and sets the review label (below). It never approves, requests changes, merges or pushes:
the user approves and merges an outside PR by hand ("make sure u leave a
comment and allow us to approve it manually"). **Never two PR reviews at
once** ("make sure we are not running concurrent PR reviews"): one PR per
`/review-prs` run, guarded by the lock `.claude/pr-review.lock` (`<N> <sha>
<UTC>`) - a run that finds a lock younger than 90 min stops; an older one is
stale and is cleared with its leftover `../cc-pr<N>-review` worktree. Our own
issue PRs are reviewed here too once flagged ready (user, 2026-11-08: "All
PR*s made by owner should be reviewd and tagged"); a fix PR merges once
that review is clean, a `feat` or breaking PR at the cut (step 1 above).
**Review labels**, one colour per state: green `merge-ready` (clean review
and mergeable), red `needs-rework` (blockers), orange `conflicts` (no
longer merges with `main`), yellow `needs-maintainer` (no blockers, but
read, not built: a new dependency or a `build.rs` / scripts / `.github`
change waits for the owner's decision). On red or orange the reviewer turns one of our
own PRs back into a draft; a fork PR is left as its contributor set it,
and the review comment says what to fix.

`fix` / `perf` / `refactor` PRs and docs-, process- and CI-only PRs (no release) merge once
reviewed by a subagent that did not write them (and, for `fix` / `perf` /
`refactor`, once their quick gate is green), without waiting for a cut;
the main session merges them (`gh pr merge --merge --delete-branch`,
retargeting any stacked child to `main` first),
never while a cut is between step 1 and its tag.

**Every cut tag is published.** A release cut is not done until its tag is
on GitHub: the tag is made on `main` after the release PR merges (step 4)
and pushed at once - `git push origin vX.Y.0` - which fires `release.yml`
and attaches the built artifact to a GitHub release. A tag that exists only
locally is an unfinished cut. Never move or re-push an existing tag; a fix
after a cut merges to `main` and ships in the next minor. Never force-push -
not `main`, not a branch with an open PR.