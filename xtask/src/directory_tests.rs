//! Directory-suite selection regressions and a real Cargo/libtest execution (#927).
//! Provenance: Cargo package layout, Rust module paths, and our changed-test gate law.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::plan::{Plan, Step};
use crate::quick::{self, Diff, Quick};
use crate::{scan, single};

#[path = "directory_edges_tests.rs"]
mod edges;

struct Fixture(PathBuf);
impl Fixture {
    fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("fixture directory");
        std::fs::write(path, text).expect("fixture file");
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const CASES: &str = "//! Gate map: covers app::operation.\npub fn helper() -> u8 { 1 }\n#[test]\nfn changed() {\n    assert_eq!(helper(), 1);\n}\n#[test]\n#[ignore = \"heavy lane\"]\nfn heavy() {}\nmod nested;\n";

fn fixture() -> Fixture {
    static N: AtomicUsize = AtomicUsize::new(0);
    let f = Fixture(std::env::temp_dir().join(format!(
        "xtask-directory-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    )));
    f.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/app\"]\nresolver = \"3\"\n",
    );
    f.write(
        "crates/app/Cargo.toml",
        "[package]\nname = \"camcraft-app\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    );
    f.write(
        "crates/app/src/lib.rs",
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn changed() {}\n}\n",
    );
    f.write("crates/app/tests/flat.rs", "#[test]\nfn flat_test() {}\n");
    f.write("crates/app/tests/group/main.rs", "//! Gate map: covers app::commands.\nmod cases;\nmod local {\n    #[test]\n    fn local_test() {}\n}\n");
    f.write("crates/app/tests/group/cases.rs", CASES);
    f.write(
        "crates/app/tests/group/cases/nested.rs",
        "#[test]\nfn changed() {}\n",
    );
    f.write(
        "crates/app/tests/group/unreferenced.rs",
        "#[test]\nfn ghost() {}\n",
    );
    f
}

fn quick(f: &Fixture, file: &str, hunk: &str, new: bool, heavy: bool) -> Plan {
    let changed = vec![file.to_string()];
    let untracked = if new { changed.clone() } else { Vec::new() };
    let hunks = |_: &str| vec![hunk.to_string()];
    let before = |file: &str| std::fs::read_to_string(f.0.join(file)).ok();
    let diff = Diff {
        changed: &changed,
        untracked: &untracked,
        hunks: &hunks,
        before: &before,
    };
    match quick::plan(&f.0, &diff, heavy).expect("quick plan") {
        Quick::Plan(p) => p,
        other => panic!("unexpected {other:?}"),
    }
}

fn single(f: &Fixture, name: &str, binary: Option<&str>, exact: bool) -> Plan {
    single::plan(
        &f.0,
        &single::Filter {
            name,
            krate: Some("app"),
            binary,
            exact,
        },
    )
    .expect("single plan")
}

fn args(p: &Plan) -> Vec<String> {
    p.steps.iter().map(|s| s.args.join(" ")).collect()
}

#[test]
fn directory_suites_are_listed_without_support_directories() {
    let f = fixture();
    f.write("crates/app/tests/common/mod.rs", "pub fn helper() {}\n");
    assert_eq!(scan::binaries(&f.0, "app"), ["flat", "group"]);
    let batch = crate::plan::run_suites(&f.0, &["app"], true);
    assert!(batch[0].args.windows(2).any(|w| w == ["--test", "group"]));
}

#[test]
fn changed_directory_function_selects_only_its_real_qualified_name() {
    let f = fixture();
    let p = quick(
        &f,
        "crates/app/tests/group/cases.rs",
        "@@ -5 +5 @@",
        false,
        false,
    );
    assert_eq!(
        args(&p),
        ["test -j 6 -p camcraft-app --test group -- --test-threads=6 --exact cases::changed"]
    );
    let p = quick(
        &f,
        "crates/app/tests/group/cases/nested.rs",
        "@@ -2 +2 @@",
        false,
        false,
    );
    assert_eq!(
        args(&p),
        [
            "test -j 6 -p camcraft-app --test group -- --test-threads=6 --exact cases::nested::changed"
        ]
    );
    let p = quick(
        &f,
        "crates/app/tests/group/main.rs",
        "@@ -5 +5 @@",
        false,
        false,
    );
    assert_eq!(
        args(&p),
        ["test -j 6 -p camcraft-app --test group -- --test-threads=6 --exact local::local_test"]
    );
}

#[test]
fn helper_new_and_deleted_module_changes_run_only_the_owning_suite() {
    let f = fixture();
    for (file, hunk, new) in [
        ("crates/app/tests/group/cases.rs", "@@ -2 +2 @@", false),
        ("crates/app/tests/group/cases.rs", "", true),
        (
            "crates/app/tests/group/unreferenced.rs",
            "@@ -2 +2 @@",
            false,
        ),
        ("crates/app/tests/group/deleted.rs", "@@ -2 +1,0 @@", false),
    ] {
        let p = quick(&f, file, hunk, new, false);
        assert_eq!(
            args(&p),
            ["test -j 6 -p camcraft-app --no-fail-fast --test group -- --test-threads=6"],
            "{file}"
        );
    }
    let p = quick(&f, "crates/app/tests/common/mod.rs", "", false, false);
    assert_eq!(
        args(&p),
        [
            "test -j 6 -p camcraft-app --no-fail-fast --lib --test flat --test group -- --test-threads=6"
        ]
    );
}

#[test]
fn directory_heavy_selection_keeps_build_only_and_release_paths() {
    let f = fixture();
    let p = quick(
        &f,
        "crates/app/tests/group/cases.rs",
        "@@ -9 +9 @@",
        false,
        false,
    );
    assert_eq!(
        args(&p),
        ["test -j 6 -p camcraft-app --no-run --test group"]
    );
    let p = quick(
        &f,
        "crates/app/tests/group/cases.rs",
        "@@ -9 +9 @@",
        false,
        true,
    );
    assert_eq!(
        args(&p),
        [
            "test --release -j 6 -p camcraft-app --test group -- --test-threads=6 --ignored --exact cases::heavy"
        ]
    );
}

#[test]
fn directory_module_gate_maps_select_the_owner() {
    let f = fixture();
    for module in ["commands", "operation"] {
        let p = quick(&f, &format!("crates/app/src/{module}.rs"), "", false, false);
        assert_eq!(
            args(&p),
            ["test -j 6 -p camcraft-app --no-fail-fast --lib --test group -- --test-threads=6"]
        );
    }
}

#[test]
fn named_directory_binary_filters_and_exact_paths_are_preserved() {
    let f = fixture();
    let p = single(&f, "changed", Some("group"), true);
    assert_eq!(
        args(&p),
        [
            "test -j 6 -p camcraft-app --test group -- --test-threads=6 --exact cases::changed cases::nested::changed"
        ]
    );
    let p = single(&f, "cases::changed", Some("group"), true);
    assert_eq!(
        args(&p),
        ["test -j 6 -p camcraft-app --test group -- --test-threads=6 --exact cases::changed"]
    );
    let p = single(&f, "local_test", None, true);
    assert_eq!(
        args(&p),
        ["test -j 6 -p camcraft-app --test group -- --test-threads=6 --exact local::local_test"]
    );
    for (name, bin) in [("changed", "flat"), ("ghost", "group")] {
        assert!(
            single::plan(
                &f.0,
                &single::Filter {
                    name,
                    krate: Some("app"),
                    binary: Some(bin),
                    exact: true
                }
            )
            .is_err()
        );
    }
    let p = single(&f, "changed", None, true);
    assert_eq!(p.steps.len(), 2); // group integration and src/lib.rs, never a --test cases target.
    assert!(p.steps[1].args.iter().any(|a| a == "--lib"));
}

fn fixture_target(root: &Path) -> PathBuf {
    // Cargo fingerprints can share a binary for identical package/target names. Isolate each input.
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"))
        .join("issue927-fixture")
        .join(root.file_name().expect("unique fixture identity"))
}

fn run_step(root: &Path, step: &Step, count: usize) {
    let target = fixture_target(root);
    eprintln!(
        "fixture root={} target={}",
        root.display(),
        target.display()
    );
    let mut command = std::process::Command::new(env!("CARGO"));
    // The fixture is noninteractive; do not allocate a Windows console for child Cargo.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let output = command
        .args(&step.args)
        .current_dir(root)
        .env("CARGO_TARGET_DIR", target)
        .output()
        .expect("real Cargo fixture execution");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("cargo {}\n{stdout}\n{stderr}", step.args.join(" "));
    assert!(output.status.success(), "Cargo fixture failed");
    assert!(
        stdout.contains(&format!("{count} passed; 0 failed")),
        "must execute the selected tests, not an empty filter: {stdout}"
    );
}

#[test]
fn directory_plans_execute_one_real_cargo_test_not_zero() {
    let f = fixture();
    let quick = quick(
        &f,
        "crates/app/tests/group/cases.rs",
        "@@ -5 +5 @@",
        false,
        false,
    );
    assert_eq!(quick.steps.len(), 1);
    run_step(&f.0, &quick.steps[0], 1);
    let named = single(&f, "cases::nested::changed", Some("group"), true);
    assert_eq!(named.steps.len(), 1);
    run_step(&f.0, &named.steps[0], 1);
    let multiple = single(&f, "changed", Some("group"), true);
    run_step(&f.0, &multiple.steps[0], 2);
}

#[test]
fn conditional_modules_do_not_claim_an_empty_exact_run() {
    let f = fixture();
    f.write(
        "crates/app/tests/group/main.rs",
        "#[cfg(any())]\nmod cases;\n",
    );
    let p = quick(
        &f,
        "crates/app/tests/group/cases.rs",
        "@@ -5 +5 @@",
        false,
        false,
    );
    assert_eq!(
        args(&p),
        ["test -j 6 -p camcraft-app --no-fail-fast --test group -- --test-threads=6"]
    );
    let filter = single::Filter {
        name: "changed",
        krate: Some("app"),
        binary: Some("group"),
        exact: true,
    };
    assert!(
        single::plan(&f.0, &filter)
            .expect_err("conditional test must refuse precision")
            .contains("conditional")
    );
}

#[test]
fn inline_path_in_non_mod_rs_uses_the_containing_module_directory() {
    let f = fixture();
    f.write(
        "crates/app/tests/group/cases.rs",
        "mod inline {\n    #[path = \"other.rs\"]\n    mod inner;\n}\n",
    );
    f.write(
        "crates/app/tests/group/cases/inline/other.rs",
        "//! Gate map: covers app::operation.\n#[test]\nfn aliased() {}\n",
    );
    let p = single(&f, "aliased", Some("group"), true);
    assert_eq!(
        args(&p),
        [
            "test -j 6 -p camcraft-app --test group -- --test-threads=6 --exact cases::inline::inner::aliased"
        ]
    );
    run_step(&f.0, &p.steps[0], 1);
}

#[test]
fn inner_file_cfg_does_not_claim_an_empty_exact_run() {
    let f = fixture();
    f.write(
        "crates/app/tests/group/cases.rs",
        &format!("#![cfg(any())]\n{CASES}"),
    );
    let p = quick(
        &f,
        "crates/app/tests/group/cases.rs",
        "@@ -6 +6 @@",
        false,
        false,
    );
    assert_eq!(
        args(&p),
        ["test -j 6 -p camcraft-app --no-fail-fast --test group -- --test-threads=6"]
    );
    let filter = single::Filter {
        name: "changed",
        krate: Some("app"),
        binary: Some("group"),
        exact: true,
    };
    assert!(
        single::plan(&f.0, &filter)
            .expect_err("inner cfg precision must refuse")
            .contains("conditional")
    );
}
