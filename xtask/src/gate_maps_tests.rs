//! Pins the quick tier's selection against the REAL suite headers (#922): a change under a crate
//! that M4.1 split out of `app` must select the app suites whose Gate map names that module.
//! Provenance: our own build law (AGENTS.md "Test the change, not everything"), #919 research.

use std::path::{Path, PathBuf};

use crate::plan::Step;
use crate::quick::{self, Diff, Quick};
use crate::scan;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().map(Path::to_path_buf).unwrap_or_default()
}

/// The `--test` binaries of the `<crate> suites` step that quick plans for one changed file.
fn selected(file: &str, krate: &str) -> Vec<String> {
    let changed = vec![file.to_string()];
    let hunks = |_: &str| -> Vec<String> { Vec::new() };
    let diff = Diff { changed: &changed, untracked: &[], hunks: &hunks, before: &|_| None };
    let Ok(Quick::Plan(p)) = quick::plan(&root(), &diff, false) else { panic!("{file} did not plan") };
    let step: Option<&Step> = p.steps.iter().find(|s| s.name == format!("{krate} suites"));
    let Some(step) = step else { return Vec::new() };
    let a = &step.args;
    eprintln!("selection {file} -> {krate}: {} suite(s)", a.iter().filter(|x| *x == "--test").count());
    a.iter().enumerate().filter(|(_, x)| *x == "--test").filter_map(|(i, _)| a.get(i + 1).cloned()).collect()
}

fn count(file: &str, krate: &str) -> usize {
    selected(file, krate).len()
}

#[test]
fn post_change_selects_the_post_dialect_suites() {
    let got = selected("crates/post/src/post/mod.rs", "app");
    assert!(got.iter().any(|b| b == "conformance"), "conformance missing from {got:?}");
    // #921 / #926: fanuc_post and the heidenhain / doosan suites live in camcraft-post, so a post change selects them there.
    let own = selected("crates/post/src/post/mod.rs", "post");
    for must in ["fanuc_post", "heidenhain_cycles", "heidenhain_block_form", "doosan_dnm_post"] {
        assert!(own.iter().any(|b| b == must), "{must} missing from {own:?}");
    }
    assert!(count("crates/post/src/post/mod.rs", "app") >= 20, "post change selects too few app suites");
}

/// A file in a submodule narrows to the suites naming that submodule, and still picks some.
#[test]
fn post_submodule_change_narrows_but_still_selects() {
    let got = selected("crates/post/src/post/engine.rs", "app");
    assert!(!got.is_empty());
    let own = selected("crates/post/src/post/engine.rs", "post");
    assert!(own.iter().any(|b| b == "heidenhain_cycles"), "{own:?}");
}

#[test]
fn sim_verify_change_selects_the_verify_suites() {
    let got = selected("crates/sim/src/verify/mod.rs", "app");
    for must in ["verify", "warned_moves"] {
        assert!(got.iter().any(|b| b == must), "{must} missing from {got:?}");
    }
    assert!(count("crates/sim/src/verify/mod.rs", "app") >= 20, "verify change selects too few app suites");
}

#[test]
fn motion_machining2d_change_selects_the_machining2d_suites() {
    let got = selected("crates/motion/src/machining2d/mod.rs", "app");
    assert!(got.iter().any(|b| b == "golden_cl"), "golden_cl missing from {got:?}");
    assert!(count("crates/motion/src/machining2d/mod.rs", "app") >= 20, "machining2d selects too few");
}

/// #1068: the operations suite generates every operation through the app layer, so it pins the
/// motion generators end to end - a change to one of them must select it.
#[test]
fn motion_generator_change_selects_the_operations_suite() {
    let map = scan::gate_map(&scan::read(&root().join("crates/app/tests/operations.rs")))
        .expect("operations.rs gate map");
    for m in ["machining2d", "machining3d", "drilling", "finishing3d", "multiaxis", "plunge_law"] {
        assert!(map.contains(&format!("motion::{m}")), "operations.rs must name motion::{m}: {map}");
    }
    let got = selected("crates/motion/src/multiaxis/mod.rs", "app");
    assert!(got.iter().any(|b| b == "operations"), "operations missing from {got:?}");
}

/// No app suite may name a module that lives in another crate as `app::<module>`.
#[test]
fn no_gate_map_names_a_moved_module_as_app() {
    let moved = [
        "post", "verify", "sim", "solid_rest", "cl", "geo2d", "stock", "tools", "orientation", "spun",
        "cutter_profile", "holder_library", "machining2d", "machining3d", "drilling", "finishing3d",
        "multiaxis", "turning", "links", "load", "plasma", "probing",
    ];
    let mut stale = Vec::new();
    let mut unmapped = Vec::new();
    for b in scan::binaries(&root(), "app") {
        let file = root().join("crates/app/tests").join(format!("{b}.rs"));
        let gm = scan::gate_map(&scan::read(&file)).unwrap_or_default();
        if gm.is_empty() {
            unmapped.push(b.clone());
        }
        for m in moved {
            let hit = gm.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':')).any(|w| {
                w == format!("app::{m}") || w == format!("camcraft_app::{m}") || w.starts_with(&format!("app::{m}::"))
            });
            if hit {
                stale.push(format!("{b}: app::{m}"));
            }
        }
    }
    assert!(stale.is_empty(), "Gate maps naming moved modules as app::<module>: {stale:?}");
    assert!(unmapped.is_empty(), "suites without a Gate map: {unmapped:?}");
}

/// #921: the tests that need nothing from `app` live in the crate that owns the behaviour, so batch
/// runs them there and quick selects them for a change in that crate (own-crate Gate maps).
#[test]
fn moved_suites_live_in_their_owner_crate_and_not_in_app() {
    let owners: [(&str, &[&str]); 5] = [
        ("post", &["heidenhain_arc_centre", "heidenhain_block_form", "heidenhain_cycles", "probe_spindle",
            "probe_spindle_stop", "comment_escape", "lathe_post_refusals", "doosan_dnm_post"]),
        ("sim", &["boundary_walls", "real_tip"]),
        ("motion", &["arcs"]),
        ("cl", &["origin_approach"]),
        ("app", &[]),
    ];
    let app = scan::binaries(&root(), "app");
    let motion = scan::binaries(&root(), "motion");
    for (krate, names) in owners {
        let have = scan::binaries(&root(), krate);
        for n in names {
            assert!(have.iter().any(|b| b == n), "{n} missing from crates/{krate}/tests");
            assert!(!app.iter().any(|b| b == n), "{n} still in crates/app/tests");
            if krate != "motion" {
                assert!(!motion.iter().any(|b| b == n), "{n} still in crates/motion/tests");
            }
            let src = scan::read(&root().join(format!("crates/{krate}/tests/{n}.rs")));
            assert!(!src.contains("camcraft_app"), "{n} imports camcraft_app");
        }
    }
}

/// #1021: a file a parent module pulls in with `#[path = "posting_index_swing.rs"] pub mod
/// index_swing;` is `app::posting::index_swing`, so the suites whose map names
/// `app::posting (index_swing)` run for it - not none, as when the file's own name was taken for
/// a top-level module `app::posting_index_swing` that no map names.
#[test]
fn a_path_included_file_selects_its_parents_submodule_suites() {
    let got = selected("crates/app/src/posting_index_swing.rs", "app");
    assert!(got.iter().any(|b| b == "index_swing"), "index_swing missing from {got:?}");
    // A union, never fewer (#1033 review): suites that name the file's own stem still run.
    for (f, must) in [
        ("posting_sim.rs", "wall_column_gouge"),
        ("posting_sim.rs", "contour_arc_sim"),
        ("posting_suppressed.rs", "rest_across_setups"),
        ("posting_plasma.rs", "plasma_gate"),
    ] {
        let got = selected(&format!("crates/app/src/{f}"), "app");
        assert!(got.iter().any(|b| b == must), "{f}: {must} missing from {got:?}");
    }
    // The other #[path] siblings of posting.rs resolve the same way and still select suites.
    for f in ["posting_mounts.rs", "posting_per_op.rs", "posting_nose.rs", "posting_sim.rs"] {
        assert!(count(&format!("crates/app/src/{f}"), "app") > 0, "{f} selects nothing");
    }
}
