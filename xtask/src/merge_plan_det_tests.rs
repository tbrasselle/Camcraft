//! Determinism of the merge planner (#1320): the same inputs in any order give the identical plan, and each
//! tie-break ends in the PR number. No git: the merge answers come from a fake `trial`. Gate map: xtask merge_plan,
//! merge_plan_out.

use std::collections::{BTreeMap, BTreeSet};

use crate::merge_plan::{self, Facts, Mergeable, Pr, order_key, shared_first, wave1_pick_key, wave2_key};

fn pr(n: u32, paths: &[&str]) -> Pr {
    Pr {
        number: n,
        title: format!("PR {n}"),
        head: format!("head{n:04}"),
        paths: paths.iter().map(|p| p.to_string()).collect(),
        mergeable: Mergeable::Clean,
        merge_bases: 0,
    }
}

/// Fake merge oracle: `base` conflicts for a PR on the base alone, `pairs` for two PRs (either order).
#[derive(Default)]
struct Fake {
    base: BTreeMap<u32, Vec<String>>,
    pairs: BTreeSet<(u32, u32)>,
}

impl Fake {
    fn clash(mut self, a: u32, b: u32) -> Fake {
        self.pairs.insert((a.min(b), a.max(b)));
        self
    }

    fn base_conflict(mut self, n: u32, files: &[&str]) -> Fake {
        self.base.insert(n, files.iter().map(|f| f.to_string()).collect());
        self
    }

    fn trial(&self, onto: &[u32], n: u32) -> Result<Option<Vec<String>>, String> {
        if onto.is_empty() {
            return Ok(self.base.get(&n).cloned());
        }
        let hit = onto.iter().any(|o| self.pairs.contains(&(*o.min(&n), *o.max(&n))));
        Ok(hit.then(|| vec!["src/x.rs".to_string()]))
    }
}

fn run(f: &Facts, fake: &Fake) -> merge_plan::Plan {
    merge_plan::plan(f, &mut |onto: &[u32], n: u32| fake.trial(onto, n)).unwrap_or_default()
}

fn facts(prs: Vec<Pr>) -> Facts {
    Facts { base_name: "main".into(), base_sha: "b".repeat(40), prs, ..Facts::default() }
}

fn wave2_numbers(p: &merge_plan::Plan) -> Vec<u32> {
    p.wave2.iter().map(|w| w.number).collect()
}

/// A busy set: docs, hot, stacked, pair clashes, a base conflict, shared history, a GitHub-only conflict, an unknown.
fn busy() -> (Facts, Fake) {
    let mut prs = vec![
        pr(11, &["docs/a.md"]),
        pr(4, &["crates/a/src/lib.rs"]),
        pr(9, &["crates/b/src/x.rs", "crates/b/src/y.rs"]),
        pr(2, &["crates/c/src/z.rs"]),
        pr(7, &["crates/d/src/w.rs"]),
        pr(5, &["crates/e/src/v.rs"]),
        pr(8, &["AGENTS.md"]),
        pr(3, &["crates/f/src/u.rs"]),
    ];
    prs[6].mergeable = Mergeable::Conflicting;
    prs[7].mergeable = Mergeable::Unknown;
    let mut f = facts(prs);
    f.before.entry(9).or_default().insert(2);
    f.before.entry(7).or_default().insert(11);
    f.shared.insert((4, 5), (2, 2));
    let fake = Fake::default().clash(9, 7).clash(5, 3).clash(4, 2).base_conflict(3, &["Cargo.lock"]);
    (f, fake)
}

/// Rotations, reversals and swaps of the input order: dozens of distinct orders.
fn orders(n: usize) -> Vec<Vec<usize>> {
    let base: Vec<usize> = (0..n).collect();
    let mut all = vec![base.clone()];
    let mut rev = base.clone();
    rev.reverse();
    all.push(rev);
    for k in 1..n {
        let mut r = base.clone();
        r.rotate_left(k);
        all.push(r);
        let mut s = base.clone();
        s.swap(0, k);
        all.push(s);
        let mut t = base.clone();
        t.reverse();
        t.rotate_right(k);
        all.push(t);
    }
    all
}

#[test]
fn shuffled_input_gives_the_identical_plan() {
    let (f, fake) = busy();
    let want = run(&f, &fake);
    assert!(!want.wave1.is_empty() && !want.wave2.is_empty(), "the fixture must exercise both waves");
    let (text, json) = (merge_plan::render(&want), merge_plan::to_json(&want));
    for order in orders(f.prs.len()) {
        let mut g = facts(order.iter().map(|i| f.prs[*i].clone()).collect());
        g.before = f.before.clone();
        g.shared = f.shared.clone();
        let got = run(&g, &fake);
        assert_eq!(got, want, "order {order:?}");
        assert_eq!(merge_plan::render(&got), text, "text, order {order:?}");
        assert_eq!(merge_plan::to_json(&got), json, "json, order {order:?}");
    }
}

#[test]
fn a_repeated_pr_number_resolves_the_same_way_in_either_order() {
    let mut a = pr(5, &["a.rs"]);
    let mut b = pr(5, &["b.rs", "c.rs"]);
    a.head = "aaaa".into();
    b.head = "bbbb".into();
    let fake = Fake::default();
    let one = run(&facts(vec![a.clone(), b.clone()]), &fake);
    let two = run(&facts(vec![b, a]), &fake);
    assert_eq!(one, two);
}

#[test]
fn wave1_tie_on_rivals_and_paths_goes_to_the_lower_number() {
    assert!(wave1_pick_key(1, 2, 3) < wave1_pick_key(1, 2, 8));
    let f = facts(vec![pr(8, &["a.rs"]), pr(3, &["b.rs"])]);
    let p = run(&f, &Fake::default().clash(3, 8));
    assert_eq!(p.wave1, vec![3]);
    assert_eq!(wave2_numbers(&p), vec![8]);
}

#[test]
fn wave1_fewer_rivals_beats_fewer_paths_and_number() {
    assert!(wave1_pick_key(1, 9, 9) < wave1_pick_key(2, 1, 1));
    // 2 clashes with 5 and 6; 5 and 6 have one rival each, so both go in and 2 waits.
    let f = facts(vec![pr(2, &["a.rs"]), pr(5, &["b.rs", "c.rs"]), pr(6, &["d.rs", "e.rs"])]);
    let p = run(&f, &Fake::default().clash(2, 5).clash(2, 6));
    assert_eq!(p.wave1, vec![5, 6]);
    assert_eq!(wave2_numbers(&p), vec![2]);
}

#[test]
fn wave1_fewer_paths_beats_the_number() {
    assert!(wave1_pick_key(1, 1, 9) < wave1_pick_key(1, 2, 1));
    let f = facts(vec![pr(3, &["a.rs", "b.rs"]), pr(8, &["c.rs"])]);
    let p = run(&f, &Fake::default().clash(3, 8));
    assert_eq!(p.wave1, vec![8]);
    assert_eq!(wave2_numbers(&p), vec![3]);
}

#[test]
fn merge_order_is_docs_then_cool_then_hot_then_small_then_number() {
    assert!(order_key(true, 5, 9, 9) < order_key(false, 0, 1, 1));
    assert!(order_key(false, 0, 9, 9) < order_key(false, 1, 1, 1));
    assert!(order_key(false, 1, 1, 9) < order_key(false, 1, 2, 1));
    assert!(order_key(false, 1, 1, 1) < order_key(false, 1, 1, 2));
    let f = facts(vec![
        pr(3, &["crates/a/src/lib.rs"]),
        pr(2, &["x.rs", "y.rs"]),
        pr(5, &["z.rs"]),
        pr(4, &["w.rs"]),
        pr(9, &["docs/n.md"]),
    ]);
    let p = run(&f, &Fake::default());
    assert_eq!(p.wave1, vec![9, 4, 5, 2, 3]);
}

#[test]
fn wave2_order_is_parents_then_fewest_files_then_number() {
    assert!(wave2_key(0, 9, 9) < wave2_key(1, 0, 1));
    assert!(wave2_key(1, 1, 9) < wave2_key(1, 2, 1));
    assert!(wave2_key(1, 1, 1) < wave2_key(1, 1, 2));
    let mut f = facts(vec![pr(3, &["a.rs"]), pr(4, &["b.rs"]), pr(7, &["c.rs"]), pr(2, &["d.rs"]), pr(9, &["e.rs"])]);
    f.before.entry(2).or_default().insert(9); // 2 is stacked on 9
    let fake = Fake::default()
        .base_conflict(3, &["f1", "f2"])
        .base_conflict(4, &["f1"])
        .base_conflict(7, &["f1"])
        .base_conflict(2, &["f1"])
        .base_conflict(9, &["f1"]);
    let p = run(&f, &fake);
    // keys: 4 (0,1) 7 (0,1) 9 (0,1) 3 (0,2) 2 (1,1)
    assert_eq!(wave2_numbers(&p), vec![4, 7, 9, 3, 2]);
}

#[test]
fn shared_history_tie_goes_to_the_lower_number_and_either_key_direction_reads_the_same() {
    assert_eq!(shared_first(3, 5, 2, 2), (3, 5));
    assert_eq!(shared_first(3, 5, 3, 2), (5, 3));
    let mut f = facts(vec![pr(5, &["a.rs"]), pr(3, &["b.rs"])]);
    f.shared.insert((3, 5), (2, 2));
    assert_eq!(run(&f, &Fake::default()).stacked.get(&5), Some(&vec![3]));
    let mut g = facts(vec![pr(5, &["a.rs"]), pr(3, &["b.rs"])]);
    g.shared.insert((5, 3), (1, 2)); // 5 has 1 own commit, 3 has 2: 5 first
    assert_eq!(run(&g, &Fake::default()).stacked.get(&3), Some(&vec![5]));
}

#[test]
fn unknown_list_is_in_number_order_whatever_the_input_order() {
    let mut x = pr(9, &["a.rs"]);
    let mut y = pr(2, &["b.rs"]);
    x.mergeable = Mergeable::Unknown;
    y.mergeable = Mergeable::Unknown;
    let p = run(&facts(vec![x, y]), &Fake::default());
    assert_eq!(p.unknown, vec![2, 9]);
}

#[test]
fn output_names_the_base_and_every_head() {
    let (f, fake) = busy();
    let p = run(&f, &fake);
    let text = merge_plan::render(&p);
    assert!(text.starts_with(&format!("base {}\n", "b".repeat(40))), "{text}");
    for n in p.titles.keys() {
        assert!(text.contains(&format!("#{n} @head{n:04}")), "#{n} missing its head in:\n{text}");
    }
    let json = merge_plan::to_json(&p);
    assert!(json.contains(&format!("\"base\":\"{}\"", "b".repeat(40))), "{json}");
    assert!(json.contains("\"number\":4,\"head\":\"head0004\""), "{json}");
    assert!(json.contains("\"kind\":\"clean\""), "{json}");
    assert!(json.contains("\"files\":[\"Cargo.lock\"]"), "{json}");
}

#[test]
fn json_escapes_quotes_backslashes_and_control_characters() {
    let mut p = pr(1, &["a.rs"]);
    p.title = "say \"hi\" \\ \n\tend".into();
    let json = merge_plan::to_json(&run(&facts(vec![p]), &Fake::default()));
    assert!(json.contains("\"title\":\"say \\\"hi\\\" \\\\ \\n\\tend\""), "{json}");
}
