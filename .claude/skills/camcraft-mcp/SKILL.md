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
  set_wcs_from_features,
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
- `get_job {}`: Dump the entire document state (setups, stock, WCS, models, tools (each row with its own `number`, or `null`, and the `posts_as` T number), operations, status), and each setup's `workholding_max_rpm` (its stated limit, or `null`).
- `undo {}`: Undo the last mutating document command.
- `redo {}`: Redo the last undone command.

### 2. Setups & Kinematics (`setup_tools.rs`, #181)
- `add_setup { "name": string, "datum_from"?: string }`: Declare a new setup and activate it. Can copy stock/WCS from an existing setup.
- `activate_setup { "name": string }`: Switch the active setup context for subsequent operations and edits.
- `set_operation_setup { "operation": string, "setup": string | null }`: Move an operation into another declared setup (`null`: the implicit first position), as the GUI's setup move does (#731). The stamp is kept, so the answer's `status` reads `ok` or `stale` by the staleness law; the moved operation joins the target's unranked tail. One undo step. Refused by name: an unknown operation or setup, a move to where it already is.
- `delete_setup { "name": string }`: Delete a declared setup with its private stock and WCS (#731); one undo restores them. Refused by name: an unknown setup, an operation or program entry still machining in it (`move or delete it first` - use `set_operation_setup`), a setup another is placed relative to.
- `set_wcs { "origin"?: [x, y, z], "stock_point"?: string, "offset"?: [dx, dy, dz], "rotation"?: [qx, qy, qz, qw] }`: Place the active setup's WCS origin and orientation quaternion.
- `set_wcs_circle_centre { "edge": { "model": string, "id": "chain:0x…" }, "rotation"?: [qx, qy, qz, qw] }`: Place the active setup's WCS origin on the centre of a picked circular edge (a hole rim or boss edge, full circle or arc), at the edge's height; returns the circle's centre, axis and diameter. Refuses a straight edge, a non-circular curve, a face or a vertex by name (#739).
- `set_wcs_from_features { "picks": [{ "model": string, "id": string, "role": "origin" | "x" | "y" | "z" }], "setup"?: string }`: Place the active setup's WCS on a frame fitted to `list_features` picks - the GUI's "Set WCS from features" law (#701, #937). Axes read in Z, X, Y priority: the first axis pick is exact, the second squared to it, the third derived right-handed. A flat face sets its outward normal, a straight edge its direction, a curved wall (face or patch) its fitted axis; the origin is a vertex, a face's centroid or a wall's centre (else the second axis pick's anchor), projected onto the first pick's plane when that is a face. One undo step. Refused by name: under two picks, a role given twice, under two axes, a pick that cannot set its role, near-parallel axes (< 5 deg), a stale id, a `setup` that is not the active one. Note: a `list_features` face row's `normal` is the plane's canonical normal, not its outward side - tell faces apart by `bounds`.
- `set_work_offset { "word": "G54" | "G55" | ... | "G59" | null, "setup"?: string }`: Set the posted G-code fixture coordinate word (G54–G59); `null` clears it.
- `load_machine { "builtin"?: string, "path"?: string, "clear"?: boolean, "setup"?: string }`: Mount a machine configuration at document or setup level: a built-in preset by name (`"VMC 3-axis"`, `"Lathe"`, the Tormach models, ...) or a `.mch` file by path; `clear: true` mounts none.
- `set_workholding_max_rpm { "max_rpm": number | null, "setup"?: string }`: State the rated maximum rpm of a setup's chuck or collet; `null` clears it. Refused with a named error: a missing or non-numeric value, zero, negative or non-finite numbers, anything under 1 rpm or over 100000 rpm, an unknown setup, and a value the setup already states. `setup` defaults to the active one. One undo step. At post time, a constant-rpm (G97) turning operation faster than the limit is refused by name, and a constant-surface-speed (G96) section's rpm ceiling is capped at it (Fanuc `G50 S`, Tormach `G96 D`).
- `edit_machine { "patch": object, "setup"?: string }`: Edit the mounted machine's definition with a JSON merge patch (RFC 7396: an object merges, `null` removes a key, any other value replaces), e.g. `{"edm": {"max_orbit_radius": 0.5}}`. Keys are those `get_job`'s `machine.definition` shows. Applies as one undo step, like the Machine window's Apply. Refused by name, changing nothing: a `patch` that is not an object, an unknown key, an insane value, no machine mounted (`load_machine` first), an unknown setup, or a `setup` that follows the document's machine (edit that one, or give the setup its own first). Without `setup`, edits the document's machine.
- `get_rotary_stack { "setup"?: string }`: Read how a setup's rotary is raised: its rotary subplates and every plate under each (top down, with thicknesses), the setup's stated `nose_to_axis` if any, and the machine's work-turning rotaries' figure. `refused` names why the index-swing check cannot judge.
- `set_setup_nose_to_axis { "setup": string, "nose_to_axis": number | null }`: State a setup's spindle-nose-to-rotary-centreline figure in mm, measured on the stack the rotary stands on; it wins over the machine file's figure less the stack. `null` clears it. One undo step; a value that is not a length over zero (or over 5000 mm) is refused.
- `set_setup_post { "post": string | null, "setup"?: string }`: Give a declared setup its own post: a built-in post name or a `.cps` path. `null` clears it, so the setup follows the document's post again. Sets the field only; posting judges whether the post serves the setup's machine. One undo step.

### 3. Fixtures & Workholding (`fixture_tools.rs`, #181)
- `add_fixture { "model": string, "setup"?: string }`: Tag an imported model as fixture steel for collision checking.
- `remove_fixture { "name": string }`: Unmark a fixture model.
- `move_fixture { "name": string, "position": [x, y, z], "rotation"?: [qx, qy, qz, qw] }`: Position a fixture body in world coordinates.
- `get_fixture_findings {}`: Query live collision checks between generated toolpaths and fixture solids / machine table.
- `locate_part { "part": string, "fixture": string, "mates"?: [{ "part", "fixture" }] (1 to 3), "offset"?: [dx, dy, dz], "turn"?: number, "move"?: "part" | "fixture" }`: Locate the part in a fixture by mates: each mate pairs a part face or vertex id with the fixture face or vertex it sits on (ids from `list_features`), in order: floor, fixed jaw, stop (3-2-1). Then an optional `offset` (mm) and `turn` (degrees about the first face mate's normal). `move: "fixture"` moves the fixture on the part instead (default `"part"`). One undo step; refusals and an interpenetration warning are named.
- `add_builtin_vise { "size": string, "setup"?: string, "fit_to_stock"?: boolean, "position"?: [x, y, z], "rotation"?: [x, y, z, w], "opening"?: number, "dimensions"?: object }`: Place a built-in generic milling vise as a fixture of a setup (the active one by default). `size` is a row of the size table (the `enum` in the tool's schema). `fit_to_stock: true` sets it against the setup's stock and opens the jaws to it, ignoring `position`, `rotation` and `opening`. Otherwise it goes at `position` and `rotation` (default the origin), with `opening` in mm (default half the maximum). `dimensions` overrides figures in mm.
- `set_vise { "name": string, "opening"?: number, "from_stock"?: boolean, "dimensions"?: object }`: Change a placed built-in vise: its jaw opening in mm, or `from_stock: true` to open the jaws to the setup's stock as the vise stands, and/or dimension overrides in mm. The mesh is regenerated; one undo step.
- `add_accessory { "kind": string, "setup"?: string, "position"?: [x, y, z], "rotation"?: [x, y, z, w], "under_stock_in_vise"?: string, "above"?: number, "steps"?: integer, "dimensions"?: object }`: Place a built-in workholding accessory as a fixture of a setup. `kind` is one of `parallels`, `block`, `step_block`, `strap_clamp`, `soft_jaw` or `rotary_subplate` (the schema's enum). `under_stock_in_vise` (parallels only) names a built-in vise fixture and fits the parallels under the stock in it, ignoring `position`, `rotation` and the height; `above` (default 6.35 mm) is how far the stock's top stands over the jaws. `steps` is the step count of a `step_block`. A `rotary_subplate` goes on a fixture plate's dowel pair with `place_on_plate`.
- `set_accessory { "name": string, "steps"?: integer, "dimensions"?: object }`: Resize a placed built-in accessory: dimension overrides in mm (and `steps` for a step block). The mesh is regenerated; one undo step.
- `add_fixture_plate { "preset": string, "setup"?: string, "on_table"?: boolean, "position"?: [x, y, z], "rotation"?: [x, y, z, w], "dimensions"?: object, "thread"?: string, "dowels"?: object | null, "counterbore"?: object | null }`: Place a built-in generic grid (tooling) plate as a fixture of a setup. Its frame's origin is the plate's corner on its top face (x along the length, y along the width, the slab below). `on_table: true` centres it on the setup machine's table, lying on its top, and ignores `position` and `rotation`. `dimensions` overrides the preset's figures in mm (`offset_x` and `offset_y` place A1 from the origin corner); `dowels` is `{pitch_x, pitch_y, offset_x, offset_y, diameter}` or `null`; `counterbore` is `{diameter, depth}` or `null`. Grid locations: rows lettered along +Y from A, columns numbered along +X from 1. One undo step; nonsense figures are refused by name.
- `set_fixture_plate { "name": string, "dimensions"?: object, "thread"?: string, "dowels"?: object | null, "counterbore"?: object | null }`: Change a placed built-in plate's figures, in the same shapes as `add_fixture_plate`. The mesh is regenerated and every fixture on it is re-placed at its named location. Refused if a location is gone, a fixture would hang off the plate, or a mod vise jaw would reach into the stock. The reply's `warning` names a re-placed fixture, or a new slab sitting in a part or the stock. One undo step.
- `detect_plate_grid { "name": string, "clear"?: boolean }`: Make an imported fixture a plate by detecting its hole grid: holes on its Z, an axis-aligned lattice, each hole snapped to its node within 0.05 mm, read in the model's own frame. Refused by name when its holes form no regular grid, or when the plate is turned in its model. `clear: true` stops it being a plate. One undo step.
- `get_plate_grid { "name": string }`: Read a plate's grid: pitch, rows and columns, the location names, gaps, the dowel lattice, and the fixtures standing on it.
- `place_on_plate { "fixture": string, "plate": string, "hole"?: string, "dowels"?: [string, string], "yaw"?: number }`: Place a fixture on a plate, either at one grid location (`hole`, e.g. `"C3"`: its origin on the hole at the plate top) or on a dowel pair (`dowels`: origin on the first location, +X toward the second). `yaw` (degrees, default 0) turns it about the plate's Z. The placement is remembered, so moving the plate moves it. A rotary subplate goes only on a dowel pair as far apart as its pin bores. Refused by name: an unknown location, a fixture that would hang off the plate, subplate pins that do not fit. One undo step.
- `add_mod_vise_pair { "plate": string, "hole": string, "size": string, "yaw"?: 0 | 90 | 180 | 270, "insert"?: "hard" | "soft", "fixed"?: object, "adjustable"?: object }`: Place a modular plate-mounted vise: two jaw blocks bolted to grid holes. The fixed block goes on `hole` (its anchor bolt), turned `yaw` degrees (default 0: its jaw faces +Y). The adjustable block, turned half a turn, goes on the same grid line at the hole nearest the setup's stock that leaves it clear, then slides along its slots until its jaw touches the stock. `size` is the preset pair; `fixed` and `adjustable` override block figures in mm. Refused by name: stock out of the slots' reach, a block off the plate, a plate with no grid, no stock, or stock in the fixed jaw or beside the jaws. One undo step.
- `set_mod_jaw { "name": string, "figures"?: object, "insert"?: "hard" | "soft" }`: Change a placed mod vise jaw block's figures or its jaw insert. `figures` takes block figures in mm, `slide` (an adjustable block's travel along its slots), `bolts` and `pins` as `[x, y]` points in the block frame. The block stays on its hole and turn. Refused by name: not a jaw block, a figure that breaks the block law, a bolt or pin off the grid, the block hanging off its plate, or its jaw face pushed into the stock. A jaw lifted off the stock comes back as `warning`. One undo step.
- `refit_mod_vise { "adjustable": string, "fixed"?: string }`: Re-fit a mod vise pair's adjustable block to the setup's stock as it stands now, after the stock changed: the fixed block stays, the adjustable block moves to the hole nearest the stock that clears it and slides until its jaw touches the stock. `fixed` names the fixed block when its plate carries more than one. Refused by name, with nothing moved: stock out of the slots' reach, stock in the fixed jaw or beside the jaws, no stock. One undo step.

### 4. Stock & Tool Management (`tools.rs`, `edit.rs`, `body_tools.rs`, #355, #361)
- `set_stock { "box"?: { "dx", "dy", "dz" }, "cylinder"?: { "diameter", "height" }, "from_model"?: string }`: Define setup stock as a box in mm (centred in XY, z from 0 to `dz`), a cylinder, or a copy of a model's mesh.
- `set_raw_stock_only { "on": boolean }`: For a stock made from a model (`set_stock` `from_model`), say whether that source model is raw stock only - a casting or blank you cut into, not the finished part. Default off: the model is judged like every model. On: the gouge verdict leaves it out. One undo step. Refused by name when there is no stock yet ("set the stock first"), when the stock is not made from a model, or when the source model has no geometry. The mark holds only while the model is still the stock's source and its geometry and placement (position and rotation) are unchanged, so moving or turning the model voids it; otherwise it is ignored and the post says so by name. An operation that machines the model (as its source or in a boundary pick) puts it back in the verdict. If the mark leaves nothing to judge, the post is refused by name.
- `list_tool_catalog {}`: List built-in tool definitions.
- `add_tool { "catalog"?: string, "definition"?: object, "name"?: string }`: Add a tool: a copy of a `list_tool_catalog` entry, or a full definition (`get_job`'s tool shape); `name` defaults to the catalog name.
- `delete_tool { "name": string }`: Delete a document tool through the undoable `DeleteTool` (one undo step; undo restores its number, holder body and cutter). Refused by tool name, changing nothing: an unknown name, an entity that is not a tool, or a tool an operation still uses (`set_tool` the operation onto another tool first).
- `set_tool_number { "tool": string, "number": integer | null }`: Give a document tool its controller number (the `T..` / `H..` a post writes, matching the machine's tool table), or `null` to go back to its list position (#730). Returns `number` (its own, or `null`) and `posts_as`. One undo step. Refused by name: an unknown or non-tool name, a number outside 1-99999, a number another tool already has, a no-op.
- `list_holder_library {}`: List the built-in holders (BT30/BT40/CAT40 with ER or Weldon, standalone ER chucks) with dimensions and the shanks each takes.
- `set_tool_holder { "tool": string, "library"?: string, "gauge_len"?: number, "projection"?: number, "points"?: [{z, r, arc?}], "clear"?: bool }`: Set a tool's holder from a library entry, a drawn spun profile, or back to the cylinder (exactly one of `library`/`points`/`clear`); one undo step. `get_job` shows it as each tool's `holder`.
- `set_tool { "operation": string, "tool": string }`: Assign a document tool to an operation.
- `get_tool_cutter_profile { "tool": string }`: Read a milling tool's cutter, neck and shank as a spun half-outline: points `[z, r, arc]` from the tip (z = 0, on the axis) up to the shank top (`arc` = 0 for a straight segment, else the signed radius of an arc to the next point). `drawn` says whether it is a hand-drawn outline or the one the tool's numbers draw. Turning tools have none.
- `set_tool_cutter_profile { "tool": string, "points": [[z, r] | [z, r, arc], ...] | null }`: Set a milling tool's drawn cutter outline (tip-up, closed to the axis at both ends), or clear it with `null` (back to the outline its numbers draw). The outline must agree with the tool's numbers (reach exactly the stated diameter within the flute length, end at flute + shank length, not exceed the cutter or shank width above the flutes, not fold over itself), else it is refused by name. Toolpaths keep reading the numbers; the collision checks and the simulation read a drawn neck where it is narrower than the stated shank (never wider). One undoable step.
- `set_work_material { "material": string | null }`: State the document's workpiece material (`aluminium_6061`, `mild_steel`, `stainless_304`, `grey_cast_iron`, `brass`, `plastics_pom`), or `null` to clear it. Re-stating the current one or an unknown word is refused by name. One undoable step; `get_job` shows it as `work_material`.
- `suggest_cutting_data { "operation": string }`: The feeds-and-speeds suggestion for the operation's tool on the stated material, as data (writes nothing): `material`, `tool_material`, `cutting_speed` m/min, `feed_per_tooth` mm, `thinning_factor`, `rpm`, `feed` mm/min, `machine_clamps`. Refused by name when no material is stated or the table cannot speak for the tool.
- `apply_cutting_data { "operation": string }`: Apply that suggestion: the operation's feed and its tool's rpm, one undoable step (the Cutting data page's Apply). Refused by name with no material, a kind with no milling feed, or an op already on the suggestion. Returns what was applied.
- `set_tool_cutter_params { "tool": string, "params"?: {diameter?, corner_r?, tip_angle?, flute_len?, shank_d?, shank_len?}, "neck"?: {diameter, length} | null }`: Edit a milling tool by the numbers its cutter outline is drawn from (mm; `tip_angle` in degrees; any subset, the others keep their value), and optionally its neck: `{diameter, length}` draws a straight neck up from the flute top, `null` drops the drawn outline (back to the outline its numbers draw), omitted keeps what is drawn. A drawn outline is carried onto the new numbers (tip regenerated from the kind, a drawn neck kept above the new flute top, shank top moved). Numbers and outline land as one undo step. Every tool-edit and cutter refusal is by name (a neck must sit below the shank's diameter and inside its length).
- `import_tool_library { "path": string, "mode"?: "add"|"update"|"replace", "holders"?: bool }`: Import tools from a `.camtools` library file as one undo step. `mode` (#899, default add) decides what a same-named document tool does: add skips an identical one and renames a different one, update takes the file's definition in place (operations go Stale), replace also removes tools the file lacks unless an operation uses one. `holders` (default true, #889) takes each tool's holder body from the file (a tool's `definition.body`, the `.camproj` tool body shape); in update/replace a changed body is gated like any change and stales the operations using it. false lands new tools with their holder cylinder only and leaves existing tools' bodies as they are. A body that is malformed or no longer fits its tool refuses the file naming the tool. The reply adds `holders` (bodies landed) and `holders_left_out`.
- `export_tool_library { "path": string }`: Export document tools to a `.camtools` library file.
- `get_tool_body { "tool": string }`: A tool's modelled body (#361), or `body: null` when it has none: a `"spindle"` body's axis, `tip_offset` and revolved envelope (`foot`, `top`, `max_radius`), or a `"lathe"` body's placement (`radial`, `along`, `tip`), `insert_zone` and `extent` from the tip. Lengths in mm.
- `set_tool_body { "tool": string, "path": string, "axis"?: [x, y, z], "tip_offset"?: number }`: Give a milling tool a body from a holder or assembly file (STEP, STL, OBJ) along the spindle axis. With `axis` (model direction, tip toward spindle) and `tip_offset` (model origin's height above the tip): numeric placement. With neither: a STEP is aligned by the ISO 13399 tool coordinate systems it names, and refused by name when it names none. The collision laws then judge the body's revolved envelope. A lathe-held tool is refused (use `set_lathe_tool_body`).
- `set_lathe_tool_body { "tool": string, "path": string, "radial": [x, y, z], "along": [x, y, z], "tip"?: [x, y, z], "insert_zone"?: { "lo": [x, y, z], "hi": [x, y, z] } | null }`: Give a lathe-held tool (turning, grooving, threading, form insert, or a boring bar) a holder body in the turning tool frame: `radial` is the model direction away from the spindle axis, `along` the model direction of the lathe's +Z, `tip` the model point the moves carry, in mm in the model's frame after import (an inch or metre STEP is scaled to mm; STL and OBJ numbers are read as mm; default the model origin). The body need not be symmetric; every move of a turning operation is then judged against the part and the standing stock, and a rub blocks the post. `insert_zone` is the insert plate's box about the tip (turning frame), which the judge leaves out.
- `set_lathe_insert_zone { "tool": string, "insert_zone": { "lo", "hi" } | null }`: Set or clear the insert zone of a tool's lathe body, keeping its mesh and placement.
- `clear_tool_body { "tool": string }`: Remove a tool's body (either frame); the holder cylinder is judged again, and a turning tool's holder goes unjudged (its operations say so).
- A body file too detailed to import at full fidelity (#864) is refused by name by every body tool: a coarse holder would read smaller than the holder is. Simplify the model and import again.

### 5. Operations & Geometric Features (`tools.rs`, `pick.rs`, `area.rs`, `features.rs`, #180, #312)
- `list_operation_types {}`: Inspect all supported operation types, parameter schemas, defaults, and compatible tool types.
- `add_operation { "kind": string, "tool"?: string, "name"?: string, "source"?: string, "params"?: object }`: Add an operation to the active setup. `source` is `"stock"` (the default) or a model name. `tool` is required except for a kind that mounts no library tool (`list_operation_types` lists `"mounts_tool": false` and a note - today the Wire Contour): omit it there; naming one is refused by the command.
- **Wire Contour (#532, #1108)**: a 2-axis wire EDM cut of one closed profile (a model's through silhouette or picked edges). Mount a wire EDM first - `load_machine { "path": "<.mch>" }` with an `edm.conditions` table (no library wire EDM exists until #533). Then `add_operation { "kind": "Wire Contour", "source": "<model>", "params": { "condition": "<condition id>", "side": "die"|"punch", "start": [x, y] } }` with no `tool` (fields: `top_z`, `bottom_z`, `condition`, `side`, `start`). `edit_operation` patches `side`, `start` and `condition`. Refusals arrive as tool errors in the command's words: a tool named, no or unknown condition, a condition with no gap or wire diameter; the start hole and the corner minimum are refused at `generate`. Not exercised on a real EDM control: the wire post is #533 and the program gate refuses a wire cut until it lands, so `post` fails by name.
- **Sinker electrode (#547, #1156)**: `electrode_from_faces { "name": string, "picks": [face ids], "undersize"?: number, "condition"?: string, "setup"?: string, "orbit"?: number, "stub": number, "pickup": number }` generates a sinker EDM electrode from the picked faces of a cavity (its level floor and every vertical wall standing on it, `face:0x…` ids from `list_features`, all on one model) and adds it as a document tool. Give `undersize` (mm per side, 0 or more) or `condition` (a machine EDM condition id, read from the setup's sinker EDM; `setup` names that setup, default the active one), not both; `stub` (more than 0) is how far the electrode stands above the cavity's mouth; `pickup` (0 or more) is the offset from the electrode's top to the measuring reference. `orbit` makes it that much smaller again per side. The reply gives the electrode, its `burn` fields (the `add_operation` params of a Sinker Burn of this cavity) a `warning` when the stated undersize is 0 (the electrode is then the cavity's full size, so any spark gap burns the walls past it), and a `note` when an orbit is given. Only an axis-aligned rectangular cavity yields an electrode; refused by name: picks on more than one model or on something that is not a face, a degenerate face, a boss, rib or overhang standing in the electrode's way, a condition with no undersize or no sinker EDM setup, a model whose facets can't be read, and malformed numbers. One undo step.
- `edit_operation { "name": string, "params": object }`: JSON merge-patch operation parameters (e.g. `stepdown`, `stepover`, `feed`, `spindle_speed`).
- `delete_operation { "name": string }`: Remove an operation and invalidate its cached toolpath.
- `rename_operation { "name": string, "new_name": string }`: Change an operation's display name (1-120 printable characters). Its stable key and its place in the program do not change; naming it back to its key clears the display name. Names are unique and cannot take another operation's key. One undo step.
- `reorder_operation { "name": string, "target": string, "place"?: "before" | "after" }`: Move an operation before (default) or after another operation of the SAME setup, as the browser's drag and drop does. Moving between setups is refused. Answers with that setup's operations in their new order. One undo step.
- `list_features { "model": string }`: Query pickable faces, loops, edge chains, and vertices with stable integer IDs.
- `get_operation_geometry { "operation": string }`: Inspect the features and area boundaries an operation machines.
- `set_operation_geometry { "operation": string, "picks": [id1, id2, ...] }`: Bind specific geometric feature IDs to an operation.
- `pick_in_box { "operation": string, "box": { "x0", "y0", "x1", "y1" }, "mode"?: "window" | "crossing", "kind"?: "chains" | "faces", "replace"?: bool, "through"?: bool }`: The viewport's box select headless (#979): seen from straight above (+Z, like the Top view), adds every feature edge chain (default) or face whose outline lies inside the world X/Y box (`window`, default) or touches it (`crossing`) to the operation's picks; `replace: true` replaces them. Without `through` (default false) the box is visible-only - hidden edges and faces under the top face stay out; `through: true` takes them too, like the Pick through toggle. Mesh triangulation edges are never taken. One undo step. Refused by name: a box with no area, a non-finite corner, a box that holds nothing new.
- `set_operation_area { "operation": string, "area": { "containment"?, "offset"?, "corner"?, "extension"?, "extra"?, "open"? } }`: Configure the area controls (containment `"inside"` | `"outside"` | `"center"`, offset, corner, ...); omitted fields take their defaults.
- `detect_holes { "name": string }`: Automatically detect and populate hole coordinates for Drill, Bore, or Thread operations from the source model.
- `detect_pockets { "model": string, "create"?: integer, "tool"?: string, "name"?: string }`: A model's recognised pockets and slots (closed or open pockets, straight slots with open, round or square ends): boundary, floor and top Z, depth, tightest inside corner, slot width and open sides. With `create` (the index of a listed feature) it adds the Pocket, or a Slot for a round or open-ended slot, that machines it, with its area and depths taken from the feature; `tool` is then required and `name` defaults to Pocket1, Slot1, and so on. One undo step.
- `set_part_axes { "y"?: picks, "x"?: picks, "flip_y"?: boolean, "flip_x"?: boolean, "primary"?: "y" | "x", "setup"?: string }`: Align the part from picked axes, as the Setups page's "Part axes" rows do (`AlignPartToSetup::align_from_axes`). Picks are `list_features` ids: one edge chain, or two points (first to second) - a point is a vertex id, or (#692) the chain id of a circular edge (a hole rim, a boss edge), read as its centre through the same picking service the GUI's centre snap uses; `flip_y` and `flip_x` reverse a pick. One axis: the part turns in the work plane so the pick runs along it (`frame: "in_plane"`, `turn_deg` reported). Both axes (#1329): their 3D directions define the full frame (`frame: "full_3d"`, `turn_deg` null) - `primary` (default `"y"`) lands exactly, the other is made square to it, and +Z lands on the right-hand cross of the picks, so a vertical edge as +Y lays the part on its side and a left-handed pair is honoured literally (the part lands on its head), not refused. Turns about the part's bounds centre, composed onto the setup's current part alignment; the work offset is untouched; one undo step. Refused by name: a face, a curved edge as the one pick, an edge in a point pair that is not circular (straight, or no circle within tolerance), coincident points, a single pick along Z, X and Y picks running (nearly) along each other.
- `lay_part_on_points { "points": [id, id, id], "setup"?: string, "plane"?: "g17" | "g18" | "g19", "flip"?: boolean, "place"?: boolean, "frame"?: boolean }`: Lay the part on a setup's G17 (XY), G18 (ZX) or G19 (YZ) plane through three picked points (`list_features` ids, exactly three): vertex ids, or (#692) circular edge chain ids, each read as its circle's centre (a straight or non-circular edge is refused by name). The plane through them is turned so its outside, the side away from the part's material, faces the plane's axis (`flip` reverses it, so on G17 the points rest on the table). `place` (default true) sets point 1 onto the plane's level. `frame` (3-2-1) also turns point 1 to 2 along +X (+Y for G19) and moves point 1 to the setup origin. One undo step.
- `lay_part_on_face { "face": {model, id}, "setup"?: string, "plane"?: "g17" | "g18" | "g19", "flip"?: boolean, "place"?: boolean }` (#729): Lay the part on a setup's G17 (XY), G18 (ZX) or G19 (YZ) plane by one picked flat face (a `list_features` face id as `{model, id}`), as the Setups page's "Lay on a plane" face pick does (`AlignPartToSetup::lay_on_plane`). The face's outward normal is turned to face the plane's axis (`flip` reverses it, so on G17 the face rests on the table) and, with `place` (default true), its centroid is set onto the plane's level. Returns `setup`, `outside`, the stored `alignment` and its `description`. One undo step. Refused by name: an edge, a vertex, a curved or degenerate face, more than one face, a fixture or pick off the part, a lay the setup already holds.

### 6. Generation, Simulation & Posting (`tools.rs`, `job.rs`, `render.rs`, #182)
- `generate { "name"?: string }`: Generate toolpaths for a named operation or all stale operations.
- `verify { "name"?: string }`: Check toolpaths against safety rules (spindle speed, rapids touching material, shaft collision, machine limits).
- `render_stock { "width"?: number, "height"?: number, "view"?: "iso" | "top" }`: Render simulated stock to a PNG image with rest/gouge color shading.
- `post { "output": string, "post"?: string }`: Post-process verified toolpaths into the NC file `output` through a built-in post by name (`"Fanuc"`, `"Haas"`, ...) or a `.cps` path.

---

## Status of Gaps from #356

When issue #356 was originally filed, several core features were GUI-only. The following capabilities have since **shipped** into the MCP tool surface:
- **Geometry picking (#180):** Shipped via `list_features`, `get_operation_geometry`, `set_operation_geometry`, and `set_operation_area`.
- **Setups, work offsets, machines, fixtures (#181):** Shipped via `add_setup`, `activate_setup`, `set_operation_setup`, `delete_setup`, `set_wcs`, `set_work_offset`, `load_machine`, `add_fixture`, `remove_fixture`, `move_fixture`, and `get_fixture_findings`.
- **Stock simulation render (#182):** Shipped via `render_stock`.
- **Hole feature detection (#312):** Shipped via `detect_holes`.
- **Tool library import/export (#355):** Shipped via `export_tool_library` and `import_tool_library`.
- **Operation display names and order (#45):** Shipped via `rename_operation` and `reorder_operation`.
- **Pocket and slot recognition (#35):** Shipped via `detect_pockets` (with `create`, it adds the Pocket or Slot that machines one).
- **WCS from feature picks (#937):** Shipped via `set_wcs_from_features` (origin/x/y/z roles over faces, edges, vertices and walls).
- **Part axes from picks (#557, #1329):** Shipped via `set_part_axes` (a straight edge or two points along +Y and/or +X; one axis turns in the work plane, both define the full 3D frame).
- **Three-point plane alignment (#555):** Shipped via `lay_part_on_points` (three vertex ids - or circular edge chain ids for their centres, #692 - onto G17, G18 or G19).
- **Face lay (#330, #729):** Shipped via `lay_part_on_face` (one flat face id onto G17, G18 or G19).
- **WCS on a circle's centre (#739):** Shipped via `set_wcs_circle_centre` (one edge chain id; the same command as the WCS page's "Circle centre" anchor).
- **Locating the part in a fixture (#561):** Shipped via `locate_part` (one to three face or vertex mates between part and fixture).
- **Live GUI session (#183, #567):** Shipped: start the app with `--mcp-port <n>` and connect with `camcraft-cli mcp --connect 127.0.0.1:<n>`; the agent's steps land on the open window's History.

---

## Known Remaining Gaps

The following capabilities exist in the GUI or app command layer but do not yet have an MCP tool:

1. **Face-to-Plane Alignment (`SetupAlign`, #330):**
   - Mating a picked face (or two edges) to a G17/G18/G19 plane is on the GUI setup page. MCP has the axis picks (`set_part_axes`, in-plane or full 3D) and the three-point lay (`lay_part_on_points`); laying a face down is `lay_part_on_face`. (The WCS itself can be placed from picks with `set_wcs_from_features`.)
2. **Document Units Lens (`SetUnits`):**
   - Toggling the document-level unit lens between millimeters and inches (`crates/app/src/commands/set_units.rs`, #578). The MCP server currently exchanges dimensions in millimeters.
3. **Persistent Program Header Metadata (#258):**
   - Setting document/setup program numbers and comments (`crates/app/src/commands/program_entry.rs`, `post_options.rs`). The MCP `post` tool currently relies on default or dialect-inferred numbers.
*Note:* Several remaining gaps above are tracked under dedicated milestone and feature issues (e.g. #258 program headers, #511 the UI's pocket recognition entry point). Per `AGENTS.md`, any future command PR that does not bundle an MCP tool must file a corresponding `source:agent` issue titled `MCP: <command>`, labelled `P5` and `area:mcp`.

---

## Rule for Contributors

Per `AGENTS.md` ("Everything Is a Command"):
> A PR that adds a user-facing command also adds its MCP tool (and script verb where it fits) in `crates/app/src/mcp/`, or files a `source:agent` follow-up issue `MCP: <command>` (labels `P5`, `area:mcp`) and names it in the PR body.
