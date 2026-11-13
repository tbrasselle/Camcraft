//! Directory selection edge cases from independent #927 review.
//! Provenance: Rust attributes/comments/module paths and our conservative quick-selection law.

use super::*;

#[test]
fn real_cargo_fixtures_have_distinct_targets() {
    let first = fixture();
    let second = fixture();
    assert_ne!(first.0, second.0);
    assert_ne!(fixture_target(&first.0), fixture_target(&second.0));
}

#[test]
fn documented_ignore_examples_are_not_ignore_attributes() {
    let f = fixture();
    for docs in [
        "/// Use `#[ignore]` for a heavy test.",
        "#[doc = \"Use #[ignore] for a heavy test.\"]",
        "#[test] // This test is not #[ignore].",
    ] {
        let text = format!("{docs}\n#[test]\nfn documented() {{}}\n");
        let tests = scan::test_fns(&text).expect("scan documentation");
        assert_eq!(tests.len(), 1);
        assert!(!tests[0].ignored, "{docs}");
        f.write("crates/app/tests/group/cases.rs", &text);
        assert_eq!(
            args(&single(&f, "documented", Some("group"), true)),
            [
                "test -j 6 -p camcraft-app --test group -- --test-threads=6 --exact cases::documented"
            ]
        );
    }
}

#[test]
fn quick_checks_old_and_new_hunk_sides_without_widening_in_test_edits() {
    let f = fixture();
    let file = "crates/app/tests/group/cases.rs";
    for (before, after, hunk, whole) in [
        (
            "fn helper() -> u8 { 1 }\n#[test]\nfn changed() { assert_eq!(helper(), 1); }\n",
            "#[allow(dead_code)] #[test]\nfn changed() { assert_eq!(2, 2); }\n",
            "@@ -1,3 +1,2 @@",
            true,
        ),
        (
            "#[test]\nfn changed() {\n    assert_eq!(1, 1);\n    assert_eq!(2, 2);\n}\n",
            "#[test]\nfn changed() {\n    assert_eq!(3, 3);\n}\n",
            "@@ -3,2 +3 @@",
            false,
        ),
    ] {
        f.write(file, after);
        let changed = vec![file.to_string()];
        let hunks = |_: &str| vec![hunk.to_string()];
        let old = |_: &str| Some(before.to_string());
        let diff = Diff {
            changed: &changed,
            untracked: &[],
            hunks: &hunks,
            before: &old,
        };
        let Quick::Plan(p) = quick::plan(&f.0, &diff, false).expect("two-sided plan") else {
            panic!("unexpected batch");
        };
        let expected = if whole {
            "test -j 6 -p camcraft-app --no-fail-fast --test group -- --test-threads=6"
        } else {
            "test -j 6 -p camcraft-app --test group -- --test-threads=6 --exact cases::changed"
        };
        assert_eq!(args(&p), [expected]);
    }
}

#[test]
fn mixed_hunks_and_deletions_run_the_owning_suite() {
    let f = fixture();
    for hunk in ["@@ -2,4 +2,4 @@", "@@ -3 +3,0 @@"] {
        let p = quick(&f, "crates/app/tests/group/cases.rs", hunk, false, false);
        assert_eq!(
            args(&p),
            ["test -j 6 -p camcraft-app --no-fail-fast --test group -- --test-threads=6"]
        );
    }
}

#[test]
fn nested_comment_braces_keep_the_real_module_path() {
    let f = fixture();
    f.write(
        "crates/app/tests/group/main.rs",
        "mod inline {\n    /* } /* nested { */ } */\n    #[test]\n    fn commented() {}\n}\n",
    );
    let p = single(&f, "commented", Some("group"), true);
    assert_eq!(
        args(&p),
        ["test -j 6 -p camcraft-app --test group -- --test-threads=6 --exact inline::commented"]
    );
    run_step(&f.0, &p.steps[0], 1);
}

#[test]
fn same_line_cfg_cannot_claim_a_precise_test() {
    let f = fixture();
    f.write(
        "crates/app/tests/group/cases.rs",
        "#[test] #[cfg(any())]\nfn off() {}\n",
    );
    let p = quick(
        &f,
        "crates/app/tests/group/cases.rs",
        "@@ -2 +2 @@",
        false,
        false,
    );
    assert_eq!(
        args(&p),
        ["test -j 6 -p camcraft-app --no-fail-fast --test group -- --test-threads=6"]
    );
    let filter = single::Filter {
        name: "off",
        krate: Some("app"),
        binary: Some("group"),
        exact: true,
    };
    assert!(
        single::plan(&f.0, &filter)
            .expect_err("compiled-out function must refuse precision")
            .contains("conditional")
    );
}

#[test]
fn ignore_before_test_still_uses_the_heavy_lane() {
    let f = fixture();
    for text in [
        "#[ignore = \"heavy lane\"]\n#[test]\nfn reversed() {}\n",
        "#[ignore = \"heavy lane\"] #[test] fn reversed() {}\n",
    ] {
        f.write("crates/app/tests/group/cases.rs", text);
        let p = single(&f, "reversed", Some("group"), true);
        assert_eq!(
            args(&p),
            [
                "test --release -j 6 -p camcraft-app --test group -- --test-threads=6 --ignored --exact cases::reversed"
            ]
        );
    }
}

#[test]
fn documentation_does_not_discard_a_module_path_attribute() {
    let f = fixture();
    f.write(
        "crates/app/tests/group/main.rs",
        "#[path = \"custom.rs\"]\n/// A test module.\nmod renamed;\n",
    );
    f.write(
        "crates/app/tests/group/custom.rs",
        "#[test]\nfn documented() {}\n",
    );
    let p = single(&f, "documented", Some("group"), true);
    assert_eq!(
        args(&p),
        ["test -j 6 -p camcraft-app --test group -- --test-threads=6 --exact renamed::documented"]
    );
}
