//! `gate test <filter>`: matching test functions in their real Cargo target.
//! Provenance: our own build law (AGENTS.md "Test the change"), Cargo layout (#927).

use std::path::Path;

use crate::plan::{ALL_CRATES, J, Plan, Step, T, pkg};
use crate::{scan, suites};

pub struct Filter<'a> {
    pub name: &'a str,
    pub krate: Option<&'a str>,
    pub binary: Option<&'a str>,
    pub exact: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Hit {
    dir: String,
    suite: bool,
    bin: String,
    func: String,
    ignored: bool,
}

type Group = (String, bool, String, bool);

/// Match either a leaf function name or its fully qualified harness path.
fn matches(name: &str, f: &Filter) -> bool {
    let leaf = name.rsplit("::").next().unwrap_or(name);
    if f.exact {
        name.eq_ignore_ascii_case(f.name) || leaf.eq_ignore_ascii_case(f.name)
    } else {
        name.contains(f.name)
    }
}

pub fn plan(root: &Path, f: &Filter) -> Result<Plan, String> {
    let dirs: Vec<String> = match f.krate {
        Some(c) => vec![c.strip_prefix("camcraft-").unwrap_or(c).to_string()],
        None => ALL_CRATES.iter().map(|d| (*d).to_string()).collect(),
    };
    let mut hits: Vec<Hit> = Vec::new();
    for d in &dirs {
        for bin in scan::binaries(root, d) {
            if f.binary.is_some_and(|b| b != bin) {
                continue;
            }
            for source in suites::sources(&suites::entry(root, d, &bin))? {
                for tf in suites::test_fns(&source)? {
                    if matches(&tf.name, f) {
                        if !source.precise {
                            return Err(format!(
                                "conditional suite test {} cannot be selected statically; run the whole {} suite",
                                tf.name, bin
                            ));
                        }
                        hits.push(Hit {
                            dir: d.clone(),
                            suite: true,
                            bin: bin.clone(),
                            func: tf.name,
                            ignored: tf.ignored,
                        });
                    }
                }
            }
        }
        if f.binary.is_none() {
            for file in scan::rs_files_recursive(&root.join("crates").join(d).join("src")) {
                for tf in scan::test_fns(&scan::read(&file))? {
                    if matches(&tf.name, f) {
                        hits.push(Hit {
                            dir: d.clone(),
                            suite: false,
                            bin: String::new(),
                            func: tf.name,
                            ignored: tf.ignored,
                        });
                    }
                }
            }
        }
    }
    if hits.is_empty() {
        return Err(format!("no #[test] fn matches '{}'", f.name));
    }
    let mut plan = Plan::default();
    plan.notes.push("matched:".into());
    for h in &hits {
        plan.notes.push(format!(
            "  {} {} {}::{}{}",
            h.dir,
            if h.suite { "test" } else { "lib" },
            h.bin,
            h.func,
            if h.ignored {
                "  [ignored -> release]"
            } else {
                ""
            }
        ));
    }
    let group = |h: &Hit| (h.dir.clone(), h.suite, h.bin.clone(), h.ignored);
    let mut groups: Vec<(Group, Vec<String>)> = Vec::new();
    for h in &hits {
        let g = group(h);
        match groups.iter_mut().find(|(k, _)| *k == g) {
            Some((_, fns)) => fns.push(h.func.clone()),
            None => groups.push((g, vec![h.func.clone()])),
        }
    }
    for ((dir, suite, bin, ignored), mut fns) in groups {
        let pk = pkg(&dir);
        let mut args: Vec<String> = vec!["test".into()];
        if ignored {
            args.push("--release".into());
        }
        for a in ["-j", J, "-p", &pk] {
            args.push(a.into());
        }
        if suite {
            args.extend(["--test".to_string(), bin.clone()]);
        } else {
            args.push("--lib".into());
        }
        args.extend(["--".to_string(), T.to_string()]);
        if ignored {
            args.push("--ignored".into());
        }
        // Integration hits carry their real module paths, so --exact cannot silently run zero.
        // Lib discovery retains its existing substring filtering (src file != module path).
        if suite {
            args.push("--exact".into());
            fns.sort();
            fns.dedup();
            args.extend(fns);
        } else {
            args.push(f.name.to_string());
        }
        let name = format!(
            "{dir} {}{}",
            if suite { bin.as_str() } else { "lib" },
            if ignored { " (ignored, release)" } else { "" }
        );
        plan.steps.push(Step::new(name, &[], &dir).with(&args));
    }
    Ok(plan)
}
