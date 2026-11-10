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
- `get_job {}`: Dump the entire document state (setups, stock, WCS, models, tools, operations, status).
- `undo {}`: Undo the last mutating document command.
- `redo {}`: Redo the last undone command.

### 2. Setups & Kinematics (`setup_tools.rs`, #181)
- `add_setup { "name": string, "datum_from"?: string }`: Declare a new setup and activate it. Can copy stock/WCS from an existing setup.
- `activate_setup { "name": string }`: Switch the active setup context for subsequent operations and edits.
- `set_wcs { "origin"?: [x, y, z], "stock_point"?: string, "offset"?: [dx, dy, dz], "rotation"?: [qx, qy, qz, qw] }`: Place the active setup's WCS origin and orientation quaternion.
- `set_work_offset { "offset": "G54" | "G55" | ... | null, "setup"?: string }`: Set the posted G-code fixture coordinate word (G54–G59).
- `load_machine { "name"?: string, "setup"?: string }`: Mount a machine configuration (`.mch` or built-in preset) at document or setup level.

### 3. Fixtures & Workholding (`fixture_tools.rs`, #181)
- `add_fixture { "model": string, "setup"?: string }`: Tag an imported model as fixture steel for collision checking.
- `remove_fixture { "name": string }`: Unmark a fixture model.
- `move_fixture { "name": string, "position": [x, y, z], "rotation"?: [qx, qy, qz, qw] }`: Position a fixture body in world coordinates.
- `get_fixture_findings {}`: Query live collision checks between generated toolpaths and fixture solids / machine table.

### 4. Stock & Tool Management (`tools.rs`, `edit.rs`, #355)
- `set_stock`: Define setup stock as a box (`{ "kind": "box", "box": [x0, y0, z0, x1, y1, z1] }`), cylinder (`{ "kind": "cylinder", ... }`), or model copy (`{ "kind": "model", "model": "part_name" }`).
- `list_tool_catalog {}`: List built-in tool definitions.
- `add_tool { "name": string, ... }`: Add a tool from catalog name or custom parameter definition.
- `set_tool { "operation": string, "tool": string }`: Assign a document tool to an operation.
- `import_tool_library { "path": string }`: Import tools from a `.camtools` library file.
- `export_tool_library { "path": string }`: Export document tools to a `.camtools` library file.

### 5. Operations & Geometric Features (`tools.rs`, `pick.rs`, `area.rs`, `features.rs`, #180, #312)
- `list_operation_types {}`: Inspect all supported operation types, parameter schemas, defaults, and compatible tool types.
- `add_operation { "kind": string, "name": string, "tool": string, "model"?: string, "params"?: object }`: Add an operation to the active setup.
- `edit_operation { "name": string, "params": object }`: JSON merge-patch operation parameters (e.g. `stepdown`, `stepover`, `feed`, `spindle_speed`).
- `delete_operation { "name": string }`: Remove an operation and invalidate its cached toolpath.
- `list_features { "model": string }`: Query pickable faces, loops, edge chains, and vertices with stable integer IDs.
- `get_operation_geometry { "operation": string }`: Inspect the features and area boundaries an operation machines.
- `set_operation_geometry { "name": string, "picks": [id1, id2, ...] }`: Bind specific geometric feature IDs to an operation.
- `set_operation_area { "name": string, "containment": "inside" | "outside" | "center", "offset"?: number }`: Configure boundary containment rules.
- `detect_holes { "name": string }`: Automatically detect and populate hole coordinates for Drill, Bore, or Thread operations from the source model.

### 6. Generation, Simulation & Posting (`tools.rs`, `job.rs`, `render.rs`, #182)
- `generate { "name"?: string }`: Generate toolpaths for a named operation or all stale operations.
- `verify { "name"?: string }`: Check toolpaths against safety rules (spindle speed, rapids touching material, shaft collision, machine limits).
- `render_stock { "width"?: number, "height"?: number, "view"?: "iso" | "top" }`: Render simulated stock to a PNG image with rest/gouge color shading.
- `post { "post"?: string, "path"?: string }`: Post-process verified toolpaths into machine NC code via `.cps` post processor.

---

## Status of Gaps from #356

When issue #356 was originally filed, several core features were GUI-only. The following capabilities have since **shipped** into the MCP tool surface:
- **Geometry picking (#180):** Shipped via `list_features`, `get_operation_geometry`, `set_operation_geometry`, and `set_operation_area`.
- **Setups, work offsets, machines, fixtures (#181):** Shipped via `add_setup`, `activate_setup`, `set_wcs`, `set_work_offset`, `load_machine`, `add_fixture`, `remove_fixture`, `move_fixture`, and `get_fixture_findings`.
- **Stock simulation render (#182):** Shipped via `render_stock`.
- **Hole feature detection (#312):** Shipped via `detect_holes`.
- **Tool library import/export (#355):** Shipped via `export_tool_library` and `import_tool_library`.

---

## Known Remaining Gaps

The following capabilities exist in the GUI or app command layer but do not yet have an MCP tool:

1. **Part Model Translation / Rotation (`MoveModel`):**
   - The GUI allows moving and turning imported models (`crates/app/src/commands/move_model.rs`). In MCP, models remain at their imported coordinates; orientation must be adjusted via `set_wcs`.
2. **High-Level Workplane Alignment (`SetupAlign`):**
   - Aligning a model by mating a face to a plane (#330), aligning axes by picking an edge (#557), or 3-point plane alignment (#555) are available in the GUI setup page. MCP currently requires computing and passing the origin and quaternion directly to `set_wcs`.
3. **Mesh Repair (`RepairModel`):**
   - Welded vertex, flipped triangle, and degenerate facet repairs (`crates/app/src/commands/repair_model.rs`) are available in the GUI but not yet exposed over MCP.
4. **Document Units Lens (`SetUnits`):**
   - Toggling the document-level unit lens between millimeters and inches (`crates/app/src/commands/set_units.rs`, #578). The MCP server currently exchanges dimensions in millimeters.
5. **Persistent Program Header Metadata (#258):**
   - Setting document/setup program numbers and comments (`crates/app/src/commands/program_entry.rs`, `post_options.rs`). The MCP `post` tool currently relies on default or dialect-inferred numbers.
6. **Automatic 2D/3D Pocket & Slot Feature Recognition (#35, #511):**
   - Automatic recognition of milled pockets and slots is supported in GUI workflows, whereas MCP currently only exposes automatic feature recognition for holes (`detect_holes`).
7. **STEP Vendor Tool Body Attachment (`SetToolBody`, #368, #370):**
   - Attaching and aligning vendor STEP models to tool holders is supported in the tool editor GUI, but lacks an MCP command wrapper.
8. **Live Interactive GUI Connection (#183, #567):**
   - `camcraft-cli mcp` currently runs as a standalone headless stdio session. Driving an already-running visible GUI window over a live socket is an open enhancement.

*Note:* Several remaining gaps above are tracked under dedicated milestone and feature issues (e.g. #183 live GUI, #258 program headers, #370 tool body alignment, #511 feature recognition). Per `AGENTS.md`, any future command PR that does not bundle an MCP tool must file a corresponding `source:agent` issue titled `MCP: <command>`.

---

## Rule for Contributors

Per `AGENTS.md` ("Everything Is a Command"):
> A PR that adds a user-facing command also adds its MCP tool (and script verb where it fits) in `crates/app/src/mcp/`, or files a `source:agent` follow-up issue `MCP: <command>` and names it in the PR body.
