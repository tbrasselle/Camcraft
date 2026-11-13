# crates/ui/AGENTS.md - steering for UI work

Addendum to the root `AGENTS.md`, which stays the law. This file gathers the
UI rules in one place for work under `crates/ui/`. It restates existing rules
and links back to where each comes from; it adds no new rule. Where this file
and the root file differ, the root file wins.

## Layering

- ui → app → flecs. Panels, widgets and dialogs do not call `sys_*` or
  world-level Flecs APIs; that work goes in app. Only app-layer systems and
  commands use raw Flecs calls. (Root `AGENTS.md`, "Dependencies".)

## Every UI change is a command

- A UI-driven state change goes through an undoable command with entity
  references, never an ad-hoc field write. Hover, drag previews and simulation
  playback stay immediate-mode and are not undoable. (Root `AGENTS.md`,
  "Everything Is a Command".)
- A UI change with no command behind it is a bug; add the command. (Same
  section.)

## Where a setup's controls live

- Controls that act on one setup live on that setup's own page (`Setup<n>`,
  `crates/ui/src/egui_app/setup_pages.rs`). The Setups list page keeps the
  setup list, New setup and the export button only. (#1104, maintainer
  decision on the question dialog, 2026-11-12.)

## Form seam (in progress)

- Rows and fields go through the form helpers in
  `crates/ui/src/egui_app/layout/form.rs` and `fields.rs`, not through raw
  `ui.button`, `ui.checkbox`, `ui.label` or `ui.add` calls in pages. The
  remaining pages move one family per PR; the work is tracked in #1122.

## File size

- No source file grows past about 400 lines. Split along the domain seam
  instead of stacking another section onto a large file. (Root `AGENTS.md`,
  "Code shape", 400-line files.)

## Tests

- UI behaviour is asserted in structural tests, not pixels:
  `crates/ui/src/egui_app/tests/structural.rs` checks AccessKit labels, roles,
  enabled/disabled state, window presence and painted text. (Root `AGENTS.md`,
  "Testing"; #255.)
- Pixel snapshot PNGs (`crates/ui/tests/snapshots/`) run only in the release
  tier's snapshot lane. A PR never changes a snapshot PNG, not to refresh a
  conflict and not to accept a new render; they are regenerated once at a
  minor or major cut. (Root `AGENTS.md`, "Testing".)

## Icons and assets

- The UI must render each asset in `assets/ui/`: loading the file is not
  enough, and each panel's draw code must use it. All icons are referenced from
  `crates/ui/src/egui_app/icons.rs`. (Root `AGENTS.md`, "Assets".)
- Icons are original, or from an MIT, ISC, Apache-2.0 or BSL set that needs no
  attribution. Tabler Icons (MIT) is the primary source. Keep each set's
  licence text in `assets/ui/LICENSES.md`. Nothing is adapted from HSMWorks or
  Fusion material. (Root `AGENTS.md`, "Assets" and "Clean-Room Protocol"; #661.)
- Nothing is taken from SolidCraft without recorded permission. Its brand
  licence (`LICENSE-BRAND`, added 2026-11-10) asks for written permission before
  its look or theme is used in another app. (#661; the licence finding in the
  #655 estimate, 2026-11-12.)
