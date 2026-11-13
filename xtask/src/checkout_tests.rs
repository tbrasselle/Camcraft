//! Pins the gate's checkout guard (#1201): the comparison of two readings of HEAD and the working tree.
//! Works on fake readings - no git is needed, so the detection logic is tested as a pure function.
//! Provenance: our own build law (AGENTS.md "Three gate tiers"), #1201.

use crate::checkout::{Reading, void_message};

fn reading(head: &str, status: &str) -> Reading {
    Reading { head: head.into(), status: status.into() }
}

const A: &str = "d7e9a807aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "2b9d33e3bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[test]
fn unchanged_checkout_has_no_void_message() {
    let r = reading(A, " M xtask/src/exec.rs\n");
    assert_eq!(void_message(&r, &r.clone(), "at the end"), None);
}

#[test]
fn head_moved_with_clean_tree_names_both_commits() {
    let msg = void_message(&reading(A, ""), &reading(B, ""), "before step 2").expect("void");
    assert!(msg.starts_with("VOID: the checkout moved from d7e9a807 to 2b9d33e3"), "{msg}");
    assert!(msg.contains("before step 2"), "{msg}");
    assert!(msg.ends_with("not a verdict on any commit"), "{msg}");
}

#[test]
fn half_moved_checkout_is_caught_with_head_unchanged() {
    // The #1201 case: HEAD still at A, the index and tree already rewritten to B's files.
    let start = reading(A, "");
    let now = reading(A, "M  crates/ui/src/a.rs\nM  crates/ui/src/b.rs\n");
    let msg = void_message(&start, &now, "at the end").expect("void");
    assert!(msg.starts_with("VOID: the working tree of d7e9a807 changed (2 entries differ"), "{msg}");
    assert!(msg.contains("M  crates/ui/src/a.rs"), "{msg}");
}

#[test]
fn head_and_tree_both_moved_say_so() {
    let start = reading(A, "");
    let now = reading(B, " M x.rs\n");
    let msg = void_message(&start, &now, "at the end").expect("void");
    assert!(msg.contains("moved from d7e9a807 to 2b9d33e3 and its working tree changed (1 entry differs:  M x.rs)"), "{msg}");
}

#[test]
fn long_diffs_are_capped_at_five_named_entries() {
    let now = reading(A, "?? a\n?? b\n?? c\n?? d\n?? e\n?? f\n?? g\n");
    let msg = void_message(&reading(A, ""), &now, "at the end").expect("void");
    assert!(msg.contains("7 entries differ"), "{msg}");
    assert!(msg.contains("and 2 more"), "{msg}");
}

#[test]
fn short_sha_is_safe_on_short_input() {
    let msg = void_message(&reading("ab", ""), &reading("cd", ""), "at the end").expect("void");
    assert!(msg.contains("from ab to cd"), "{msg}");
}
