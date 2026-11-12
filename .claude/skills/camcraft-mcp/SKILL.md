---
name: camcraft-mcp
description: Guide and reference for driving CamCraft headlessly via its Model Context Protocol (MCP) server (camcraft-cli mcp). Covers the agent workflow, tool table, parameter DTO shapes, undo/redo, error handling, shipped capabilities, and remaining gaps.
---

# CamCraft MCP Agent Guide

CamCraft exposes a Model Context Protocol (MCP) server on stdio (`camcraft-cli mcp`, #128), allowing an AI agent to drive a complete CAM programming session headlessly: importing models, defining setups and stock, selecting tools, creating and editing operations, selecting geometric features, simulating/verifying, and posting NC programs.

## Architecture & Principles

- **Transport & Protocol:** Standard JSON-RPC 2.0 over newline-delimited stdio (`initialize`, `tools/list`, `tools/call`, `ping`).
- **Everything Is a Command:** The MCP server owns one headless document (`crates/app/src/script/mod.rs:Env`) and one command `History`. Every mutation is dispatched through a concrete undoable command (`env.history.execute`), guaranteeing that `undo` and `redo` work headlessly with the exact same invariants as the GUI.
- **Refusals as Data & Prime Directive:** All tool failures return structured refusals: `{ isError: true, content: [{ "type": "text", "text": "<tool_name>: <reason>" }] }` naming the exact parameter or physical constraint violated. In accordance with CamCraft's Prime Directive (no panics in user-triggered code; crashes are bugs), systems and commands return explicit `Result`/`Option` errors. The `catch_unwind` wrapper in `crates/app/src/mcp/mod.rs` serves strictly as a cold, last-resort crash barrier to prevent an unexpected internal engine defect from dropping the stdio connection.

---

## Standard Agent Workflow

```
       1. Import / Open
   (import_model / open_project)
              │
              ▼
    2. Stock, Setup & WCS
 (set_stock, add_setup, set_wcs,
  set_work_offset, load_machine)
              │
              ▼
         3. Fixtures
 (add_fixture, move_fixture)
              │
              ▼
          4. Tooling
 (add_tool, import_tool_library)
              │
              ▼
   5. Operations & Geometry
 (add_operation, list_features,
  set_operation_geometry, edit_operation)
              │
              ▼
         6. Generate
         (generate)
              │
              ▼
     7. Verify & Inspect
 (verify, get_fixture_findings,
         render_stock)
              │
              ▼
          8. Post
          (post)
```

---

## Tool Reference

The server exposes tools across three modules in `crates/app/src/mcp/` (see `crates/app/src/mcp/tools.rs:all()` or the `tools/list` protocol method for the authoritative live registry):

### 1. Document Lifecycle & History (`tools.rs`)
- `open_project { "path": string }`: Load an existing `.camproj` file, replacing the current document and clearing history.
- `save_project { "path"?: string }`: Save the current document to disk. Without a path, overwrites the last opened/saved file.
- `import_model { "path": string, "name"?: string }`: Import a mesh model (STEP, STL, OBJ). Re-importing with an existing name updates that model.
- `move_model { "name": string, "position": [x, y, z], "rotation"?: [x, y, z, w] }`: Place and turn an imported part (mm, unit quaternion; omitted rotation keeps the current one). Undoable; reports where stock that follows the part ended up (#716).
- `repair_model { "name": string, "weld_tol"?: number, "max_hole_edges"?: integer }`: Weld vertices (default 0.001 mm), drop degenerate or duplicate triangles and fill open loops of at most `max_hole_edges` edges (default 256); reports the counts (#717).
- `get_job {}`: Dump the entire document state (setups, stock, WCS, models, tools, operations, status), and each setup's `workholding_max_rpm` (its stated limit, or `null`).
- `undo {}`: Undo the last mutating document command.
- `redo {}`: Redo the last undone command.

### 2. Setups & Kinematics (`setup_tools.rs`, #181)
- `add_setup { "name": string, "datum_from"?: string }`: Declare a new setup and activate it. Can copy stock/WCS from an existing setup.
- `activate_setup { "name": string }`: Switch the active setup context for subsequent operations and edits.
- `set_wcs { "origin"?: [x, y, z], "stock_point"?: string, "offset"?: [dx, dy, dz], "rotation"?: [qx, qy, qz, qw] }`: Place the active setup's WCS origin and orientation quaternion.
- `set_work_offset { "word": "G54" | "G55" | ... | "G59" | null, "setup"?: string }`: Set the posted G-code fixture coordinate word (G54–G59); `null` clears it.
- `load_machine { "builtin"?: string, "path"?: string, "clear"?: boolean, "setup"?: string }`: Mount a machine configuration at document or setup level: a built-in preset by name (`"VMC 3-axis"`, `"Lathe"`, the Tormach models, ...) or a `.mch` file by path; `clear: true` mounts none.
- `set_workholding_max_rpm { "max_rpm": number | null, "setup"?: string }`: State the rated maximum rpm of a setup's chuck or collet; `null` clears it. Refused with a named error: a missing or non-numeric value, zero, negative or non-finite numbers, anything under 1 rpm or over 100000 rpm, an unknown setup, and a value the setup already states. `setup` defaults to the active one. One undo step. At post time, a constant-rpm (G97) turning operation faster than the limit is refused by name, and a constant-surface-speed (G96) section's rpm ceiling is capped at it (Fanuc `G50 S`, Tormach `G96 D`).

### 3. Fixtures & Workholding (`fixture_tools.rs`, #181)
- `add_fixture { "model": string, "setup"?: string }`: Tag an imported model as fixture steel for collision checking.
- `remove_fixture { "name": string }`: Unmark a fixture model.
- `move_fixture { "name": string, "position": [x, y, z], "rotation"?: [qx, qy, qz, qw] }`: Position a fixture body in world coordinates.
- `get_fixture_findings {}`: Query live collision checks between generated toolpaths and fixture solids / machine table.

### 4. Stock & Tool Management (`tools.rs`, `edit.rs`, #355)
- `set_stock { "box"?: { "dx", "dy", "dz" }, "cylinder"?: { "diameter", "height" }, "from_model"?: string }`: Define setup stock as a box in mm (centred in XY, z from 0 to `dz`), a cylinder, or a copy of a model's mesh.
- `set_raw_stock_only { "on": boolean }`: For a stock made from a model (`set_stock` `from_model`), say whether that source model is raw stock only - a casting or blank you cut into, not the finished part. Default off: the model is judged like every model. On: the gouge verdict leaves it out. One undo step. Refused by name when there is no stock yet ("set the stock first"), when the stock is not made from a model, or when the source model has no geometry. The mark holds only while the model is still the stock's source and its geometry and placement (position and rotation) are unchanged, so moving or turning the model voids it; otherwise it is ignored and the post says so by name. An operation that machines the model (as its source or in a boundary pick) puts it back in the verdict. If the mark leaves nothing to judge, the post is refused by name.
- `list_tool_catalog {}`: List built-in tool definitions.
- `add_tool { "catalog"?: string, "definition"?: object, "name"?: string }`: Add a tool: a copy of a `list_tool_catalog` entry, or a full definition (`get_job`'s tool shape); `name` defaults to the catalog name.
- `list_holder_library {}`: List the built-in holders (BT30/BT40/CAT40 with ER or Weldon, standalone ER chucks) with dimensions and the shanks each takes.
- `set_tool_holder { "tool": string, "library"?: string, "gauge_len"?: number, "projection"?: number, "points"?: [{z, r, arc?}], "clear"?: bool }`: Set a tool's holder from a library entry, a drawn spun profile, or back to the cylinder (exactly one of `library`/`points`/`clear`); one undo step. `get_job` shows it as each tool's `holder`.
- `set_tool { "operation": string, "tool": string }`: Assign a document tool to an operation.
- `get_tool_cutter_profile { "tool": string }`: Read a milling tool's cutter, neck and shank as a spun half-outline: points `[z, r, arc]` from the tip (z = 0, on the axis) up to the shank top (`arc` = 0 for a straight segment, else the signed radius of an arc to the next point). `drawn` says whether it is a hand-drawn outline or the one the tool's numbers draw. Turning tools have none.
- `set_tool_cutter_profile { "tool": string, "points": [[z, r] | [z, r, arc], ...] | null }`: Set a milling tool's drawn cutter outline (tip-up, closed to the axis at both ends), or clear it with `null` (back to the outline its numbers draw). The outline must agree with the tool's numbers (reach exactly the stated diameter within the flute length, end at flute + shank length, not exceed the cutter or shank width above the flutes, not fold over itself), else it is refused by name. Display only: toolpaths and checks keep reading the numbers. One undoable step.
- `import_tool_library { "path": string, "mode"?: "add"|"update"|"replace", "holders"?: bool }`: Import tools from a `.camtools` library file as one undo step. `mode` (#899, default add) decides what a same-named document tool does: add skips an identical one and renames a different one, update takes the file's definition in place (operations go Stale), replace also removes tools the file lacks unless an operation uses one. `holders` (default true, #889) takes each tool's holder body from the file (a tool's `definition.body`, the `.camproj` tool body shape); in update/replace a changed body is gated like any change and stales the operations using it. false lands new tools with their holder cylinder only and leaves existing tools' bodies as they are. A body that is malformed or no longer fits its tool refuses the file naming the tool. The reply adds `holders` (bodies landed) and `holders_left_out`.
- `export_tool_library { "path": string }`: Export document tools to a `.camtools` library file.

### 5. Operations & Geometric Features (`tools.rs`, `pick.rs`, `area.rs`, `features.rs`, #180, #312)
- `list_operation_types {}`: Inspect all supported operation types, parameter schemas, defaults, and compatible tool types.
- `add_operation { "kind": string, "tool": string, "name"?: string, "source"?: string, "params"?: object }`: Add an operation to the active setup. `source` is `"stock"` (the default) or a model name.
- `edit_operation { "name": string, "params": object }`: JSON merge-patch operation parameters (e.g. `stepdown`, `stepover`, `feed`, `spindle_speed`).
- `delete_operation { "name": string }`: Remove an operation and invalidate its cached toolpath.
- `list_features { "model": string }`: Query pickable faces, loops, edge chains, and vertices with stable integer IDs.
- `get_operation_geometry { "operation": string }`: Inspect the features and area boundaries an operation machines.
- `set_operation_geometry { "operation": string, "picks": [id1, id2, ...] }`: Bind specific geometric feature IDs to an operation.
- `set_operation_area { "operation": string, "area": { "containment"?, "offset"?, "corner"?, "extension"?, "extra"?, "open"? } }`: Configure the area controls (containment `"inside"` | `"outside"` | `"center"`, offset, corner, ...); omitted fields take their defaults.
- `detect_holes { "name": string }`: Automatically detect and populate hole coordinates for Drill, Bore, or Thread operations from the source model.

### 6. Generation, Simulation & Posting (`tools.rs`, `job.rs`, `render.rs`, #182)
- `generate { "name"?: string }`: Generate toolpaths for a named operation or all stale operations.
- `verify { "name"?: string }`: Check toolpaths against safety rules (spindle speed, rapids touching material, shaft collision, machine limits).
- `render_stock { "width"?: number, "height"?: number, "view"?: "iso" | "top" }`: Render simulated stock to a PNG image with rest/gouge color shading.
- `post { "output": string, "post"?: string }`: Post-process verified toolpaths into the NC file `output` through a built-in post by name (`"Fanuc"`, `"Haas"`, ...) or a `.cps` path.

---

## Status of Gaps from #356

When issue #356 was originally filed, several core features were GUI-only. The following capabilities have since **shipped** into the MCP tool surface:
- **Geometry picking (#180):** Shipped via `list_features`, `get_operation_geometry`, `set_operation_geometry`, and `set_operation_area`.
- **Setups, work offsets, machines, fixtures (#181):** Shipped via `add_setup`, `activate_setup`, `set_wcs`, `set_work_offset`, `load_machine`, `add_fixture`, `remove_fixture`, `move_fixture`, and `get_fixture_findings`.
- **Stock simulation render (#182):** Shipped via `render_stock`.
- **Hole feature detection (#312):** Shipped via `detect_holes`.
- **Tool library import/export (#355):** Shipped via `export_tool_library` and `import_tool_library`.
- **Pocket and slot recognition (#35):** Shipped via `detect_pockets` (with `create`, it adds the Pocket or Slot that machines one).
- **Part axes in the work plane (#557):** Shipped via `set_part_axes` (a straight edge or two points along +Y and/or +X).
- **Three-point plane alignment (#555):** Shipped via `lay_part_on_points` (three vertex ids onto G17, G18 or G19).
- **Locating the part in a fixture (#561):** Shipped via `locate_part` (one to three face or vertex mates between part and fixture).
- **Live GUI session (#183, #567):** Shipped: start the app with `--mcp-port <n>` and connect with `camcraft-cli mcp --connect 127.0.0.1:<n>`; the agent's steps land on the open window's History.

---

## Known Remaining Gaps

The following capabilities exist in the GUI or app command layer but do not yet have an MCP tool:

1. **Face-to-Plane Alignment (`SetupAlign`, #330):**
   - Mating a picked face (or two edges) to a G17/G18/G19 plane is on the GUI setup page. MCP has the in-plane axes (`set_part_axes`) and the three-point lay (`lay_part_on_points`); laying a face down still means picking three of its vertices or computing the origin and quaternion for `set_wcs`.
2. **Document Units Lens (`SetUnits`):**
   - Toggling the document-level unit lens between millimeters and inches (`crates/app/src/commands/set_units.rs`, #578). The MCP server currently exchanges dimensions in millimeters.
3. **Persistent Program Header Metadata (#258):**
   - Setting document/setup program numbers and comments (`crates/app/src/commands/program_entry.rs`, `post_options.rs`). The MCP `post` tool currently relies on default or dialect-inferred numbers.
4. **STEP Vendor Tool Body Attachment (`SetToolBody`, #368, #370):**
   - Attaching and aligning vendor STEP models to tool holders is supported in the tool editor GUI, but lacks an MCP command wrapper.

*Note:* Several remaining gaps above are tracked under dedicated milestone and feature issues (e.g. #258 program headers, #370 tool body alignment, #511 the UI's pocket recognition entry point). Per `AGENTS.md`, any future command PR that does not bundle an MCP tool must file a corresponding `source:agent` issue titled `MCP: <command>`, labelled `P5` and `area:mcp`.

---

## Rule for Contributors

Per `AGENTS.md` ("Everything Is a Command"):
> A PR that adds a user-facing command also adds its MCP tool (and script verb where it fits) in `crates/app/src/mcp/`, or files a `source:agent` follow-up issue `MCP: <command>` (labels `P5`, `area:mcp`) and names it in the PR body.
