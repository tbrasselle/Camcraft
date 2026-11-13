---
name: work-issue
description: Work ONE claimed-or-free GitHub issue of NeoTech/Camcraft end to end as an issue worker - check the claim, claim it, branch a worktree from the latest main, fix it with a pinning test, run the quick gate, push and open a draft PR, move the issue to in-review. Escalates the issue's effort label and releases it when the work is bigger than the worker. Used by the issue-worker-<effort> agents (AGENTS.md "Every issue is weighted by effort"); never merges.
---

# work-issue - one issue, worked by the agent its effort calls for

You are an issue worker. The prompt names one issue `N` and your tier
(`low`, `medium`, `high` or `max`). AGENTS.md is the law; this skill is the
order of the steps. Load the `gh-issues` skill for the `gh` mechanics.

## 0. Before you start
- Read the issue and all its comments: `gh issue view N --comments`.
- **Stop without claiming** (one-line report to the caller) if any holds:
  - it carries `P?`, `needs-research`, `blocked` or `in-review`;
  - a live claim exists (AGENTS.md "Claim before any code", "A claim
    comment is a claim");
  - its effort label is above your tier - say "N is effort:<x>, not mine";
  - it has no effort label: set one from the AGENTS.md table (respecting
    the safety floor) with a one-line comment why, and stop if it is
    above your tier.
- A release cut in progress (an open `release/*` PR): stop.

## 1. Claim and branch
- `gh issue edit N --add-label claimed --add-assignee @me` and
  `gh issue comment N --body "claimed: issue-worker-<tier> (<model>) · <UTC> · worktree/branch issue/N-<slug> · plan: <one line>"`.
- In a fork, follow AGENTS.md's fork sync first (process files, cut tag).
- A worktree agent never switches, pulls, merges, resets, checks out or
  stashes in the main checkout. Its only git there is
  `git fetch origin && git worktree add --no-track -b issue/N-<slug> .claude/worktrees/issue-N origin/main`.
  Reason: the main checkout may be running a gate, which reads HEAD and the
  tree at start and end and voids the run if they moved (#1201); a worker's
  pull would void that run for everyone. Work only in the worktree, with the worktree's own default `target/` (not a `CARGO_TARGET_DIR` env var: sccache hashes it, so the shared compile cache would miss across worktrees; AGENTS.md "Testing"). Never build
  in the main checkout. Check free disk first (10-20 GB per target).
- Build cache: Prefer `cargo xtask gate ...` (it turns the compile cache on); for a direct cargo call set `RUSTC_WRAPPER=sccache` only when `sccache` is on PATH (cargo fails if the wrapper is missing), and never export `CARGO_TARGET_DIR`.

## 2. Fix
- Smallest change that meets the issue's "Done means". Follow AGENTS.md:
  the Prime Directive (no `unwrap`/`expect`/`panic!` outside tests; refuse
  by name), everything is a command (+ MCP tool or a filed `MCP:` issue),
  the 400-line law, layering, provenance line on new modules, no snapshot
  PNG changes, and the three placement rules (AGENTS.md "Code shape" and
  "Testing"): a new test lives in the lowest crate that provides its imports;
  no new `camcraft-*` dependency without a `docs/decisions/` entry listing
  the items and what rebuilds; new code in `crates/app/src` only reads or
  writes the ECS world, project file, a command, MCP or the CLI.
- A pinning test for the change; a decision taken unattended goes in
  `docs/decisions/N-<slug>.md`.

## 3. Quick gate
- One command on every OS: `cargo xtask gate quick` (single tests:
  `cargo xtask gate test <name> [--crate c] [--exact]`; `--dry-run` shows
  the plan). The old `scripts/gate.ps1 -Tier quick` / `scripts/gate.sh
  quick` still work - they forward to it. Or run the touched
  test binaries one by one, plus
  `cargo clippy -p <touched crates> --all-targets -- -D warnings`.
  Paths in commands use `/`; never assume a `.exe` suffix or a drive.
  UI suites with CWD `crates/ui`. Run test executables only from cargo's
  JSON artifact list - never glob `*.exe` (it launches the GUI).

## 4. Escalate instead of flailing
Stop and escalate when the quick gate fails **twice** after your fixes, or
the work turns out bigger than your tier (more crates, a new algorithm,
something under the safety floor that your label didn't account for):
- Commit any useful work to the branch and push it (or delete an empty
  branch: `git push origin --delete issue/N-<slug>`).
- Raise the label one step (`gh issue edit N --remove-label effort:<x> --add-label effort:<next>`);
  `max` cannot rise - swap `claimed` for `blocked` instead.
- Remove `claimed` and your assignment; comment what you learned, what
  failed (test names, error lines), and where the held work lives.
- Report "escalated N to effort:<next>" to the caller and stop. One
  escalation per run; never lower a label.

## 5. Done on the branch
- Commit (`<type>(<scope>): <subject>`, body ends `Fixes #N`, plus the
  session's Co-Authored-By line), push, open a **draft** PR with
  `--body-file`: what changed, how tested (pinning tests named), what was
  not exercised, `Changelog:` line, `Effort: <tier>`, `Fixes #N`.
- Swap `claimed` for `in-review`; comment
  `fix on issue/N-<slug> · PR #M · pinned by <tests> · merges when reviewed`.
- Remove the worktree and its target dir.
- Report to the caller: PR number, tests, anything not exercised. Never
  mark the PR ready, merge it, or review it yourself - the main session
  decides.
