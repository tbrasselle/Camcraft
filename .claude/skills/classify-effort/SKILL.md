---
name: classify-effort
description: Set or check the `effort:low|medium|high|max` label on NeoTech/Camcraft issues, by AGENTS.md's effort table and safety floor. `/classify-effort <N>` classifies one issue; `/classify-effort` labels every open issue that has no effort label; `/classify-effort audit` reports open issues whose label looks wrong without changing them. Use when filing or triaging an issue, when an issue has no effort label, before dispatching an issue-worker, or when asked to "classify effort", "weight this issue", "what effort is #N", "label the unlabelled issues".
---

# classify-effort - one effort label per issue, by the table

The effort label says how hard the **work** is. It picks the worker agent
(`.claude/agents/issue-worker-<effort>.md`) and so the model and the token
spend; `P*` still decides which issue is worked next. AGENTS.md ("Every
issue is weighted by effort") is the law; this skill is how to apply it
the same way every time. Load the `gh-issues` skill for the `gh` mechanics.

## What you may change

- **Set** a label on an issue that has none (filing, triage, the sweep).
- **Never lower** an existing label - only the maintainer does. If one
  looks too high, say so (audit mode, or one issue comment); don't edit.
- **Raise** an existing label only through `work-issue`'s escalation (a
  worker that hit the limit), one step, with its comment. Not here.
- **Never touch** an issue labelled `P?` (paused) - read it, nothing more.
- A `needs-research` umbrella is not classified here (leave whatever it
  carries); its sub-issues are, when they are filed.
- Exactly one `effort:*` label per issue. Two is a bug: report it.

## How to classify (in this order)

1. **Read the whole issue**: `gh issue view N --comments`. The body gives
   the ask and "Done means"; the comments give what is already done, what
   earlier attempts found, and any split into sub-issues.

2. **Classify the work that is left, not the title.** If earlier PRs
   merged parts of it, weigh only what remains. If the issue is an
   umbrella worked in bounded stages (each PR `Refs #N`), weigh the next
   stage a worker would take, not the whole programme (maintainer,
   2026-11-11: four old roadmap issues labelled `max` were moved to `high`
   because the work actually handed out was a bounded stage, not open
   design). The safety floor (step 4) applies to what remains just the
   same: a remaining stage that touches posted G-code, a collision or
   gouge check, stock simulation or geometry is at least `effort:high`,
   however small it is.

3. **Match it to the table row it most resembles:**

   | Label | The work | Typical signs |
   |---|---|---|
   | `effort:low` | mechanical, one file, no new logic | docs, a label or string, a layout tweak, a test fixture or field fix |
   | `effort:medium` | one module or crate, a known pattern | a UI page change, an MCP tool over an existing command, a new machine file, a release-script edit that reuses existing code |
   | `effort:high` | several crates, new behaviour with its tests | a new undoable command with its MCP tool, simulation or post logic, a refusal law, a bounded stage of a large programme |
   | `effort:max` | new algorithms, formats or open design | a new geometry or toolpath algorithm, a new collision law, a new file format, research-shaped work, a piece whose design is still open |

   The row whose "The work" column fits decides. Only when two adjacent
   rows both seem to fit, break the tie with these, in this order (the
   first that answers wins):
   - Does one piece of it need an algorithm or format that does not exist
     in the tree and has to be designed from literature? Then `max`;
     otherwise never `max`.
   - Does it add new behaviour that needs new tests of its own (not a
     copy of an existing page, tool, post or machine file)? Then `high`,
     even inside one crate.
   - Otherwise: one module or crate following an existing pattern is
     `medium`; one file with no new logic is `low`.

4. **Apply the safety floor.** At least `effort:high`, however small the
   diff, when the work can change: posted G-code, a collision or gouge
   check, stock simulation, or geometry. Every `P1` is at least
   `effort:high`. The floor only raises; it never lowers.

5. **Stop there.** These are *not* reasons to go up a row:
   - it is hard to test, or the area "feels risky" (release pipeline,
     build scripts) but is outside the safety floor;
   - the issue text is long, or the issue is old;
   - earlier attempts failed - at first classification that calls for a
     better brief or a maintainer decision, not automatically a bigger
     model (a worker that fails its gate twice still escalates one step
     through `work-issue`; that rule is separate and stands);
   - "to be safe". Inflating the label spends the expensive model on
     every such issue.
   And these are not reasons to go down: the diff is small (the floor
   still applies), or the fix "looks obvious" in safety-floor code.

6. **If it is too big for any single run** (several independent
   deliverables, open questions the maintainer must answer), the right
   move is not `max`: propose a split into sub-issues, or `needs-research`
   for the maintainer to confirm, in an issue comment.

## Writing the label

```bash
gh label list --limit 200 | grep '^effort:'        # the four must exist
gh issue edit N --add-label "effort:LEVEL"         # LEVEL = low, medium, high or max
gh issue comment N --body-file - <<'EOF'
effort:LEVEL - REASON
EOF
gh issue view N --json labels --jq '[.labels[].name]'   # verify
```

Replace `N`, `LEVEL` and `REASON` yourself; they are placeholders, not
shell syntax. `REASON` is one line: the table row the work matches, and
"safety floor" if the floor set it. The heredoc keeps backticks and `$`
in the reason from being run by the shell.

The one-line reason is required: it is what lets the table be
recalibrated later (AGENTS.md "Measured").

## Modes

### `/classify-effort <N>` - one issue
Steps 1-6, then write the label if the issue has none. If it already has
one: report whether you agree, and change nothing.

### `/classify-effort` - the sweep
```bash
gh issue list --state open --limit 400 \
  --search '-label:effort:low -label:effort:medium -label:effort:high -label:effort:max -label:"P?" -label:needs-research' \
  --json number,title,labels --jq '.[] | "\(.number)\t\(.title)"'
```
Classify each (steps 1-6). Always print the list with the proposed label
and reason before applying any of it, then label them. An issue that is
`claimed` or `in-review` is still labelled (the label is owed at triage);
it changes nothing for the work in flight. The query leaves out `P?` and
`needs-research` issues, so the sweep never sees them: say in the report
that they were excluded, not that there were none. Report a table at the
end: issue, label, reason.

### `/classify-effort audit` - read-only
For every open issue that has an effort label (skip `P?`), apply steps
1-5 and list only the disagreements: issue, current label, the label the
table gives, the reason. Change nothing; the maintainer decides. Group
the list as "looks too high" (token spend) and "below the safety floor"
(the second kind is the urgent one - say so first).

## Report

One line per issue: `#N effort:<x> - <reason>` (and `set` / `already
set, agree` / `already set, would be <y>`). Name any issue you skipped
and why. In the sweep, `P?` and `needs-research` issues are excluded by
the query; an issue with two effort labels only shows up in `audit`.
