//! The merge plan as text (what the merge-ready skill reads) and as JSON (`--json`, for other tools) (#1320).
//! Both carry the base sha and every PR's head sha, so two runs can be compared. Provenance: the reference
//! planner's output format (plan_merge_order.py, #1221), with the shas added.

use std::fmt::Write as _;

use crate::merge_plan::{Plan, Refresh, refs, short};

fn title(p: &Plan, n: u32) -> String {
    p.titles.get(&n).map(|t| t.chars().take(80).collect()).unwrap_or_default()
}

fn head(p: &Plan, n: u32) -> String {
    p.heads.get(&n).map(|h| short(h)).unwrap_or_default()
}

/// The plan as the text the skill reads.
pub fn render(p: &Plan) -> String {
    let mut s = format!("base {}\n", p.base_sha);
    if !p.unknown.is_empty() {
        let _ = writeln!(s, "WARNING: GitHub has not computed mergeability yet for {} - run again before trusting wave 1", refs(&p.unknown));
    }
    let _ = writeln!(s, "\nWAVE 1 - merge in this order; no head moves, so no refresh and no delta review ({}):", p.wave1.len());
    for n in &p.wave1 {
        let dep = p.stacked.get(n).map(|a| format!("  (after {})", refs(a))).unwrap_or_default();
        let _ = writeln!(s, "  #{n} @{}  {}{dep}", head(p, *n), title(p, *n));
    }
    if !p.wave2.is_empty() {
        let _ = writeln!(
            s,
            "\nWAVE 2 - one at a time: refresh against the main of that moment, check, merge, THEN the next ({}):",
            p.wave2.len()
        );
        for w in &p.wave2 {
            let mut files = w.files.iter().take(6).cloned().collect::<Vec<_>>().join(", ");
            if w.files.len() > 6 {
                files.push_str(" ...");
            }
            let files = if files.is_empty() { files } else { format!(": {files}") };
            let _ = writeln!(s, "  #{} @{}  [{}] {}{files}\n         {}", w.number, head(p, w.number), w.kind.label(), w.why, title(p, w.number));
        }
    }
    s
}

/// A JSON string literal (quotes, backslashes and control characters escaped).
fn jstr(v: &str) -> String {
    let mut s = String::from("\"");
    for c in v.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            '\r' => s.push_str("\\r"),
            '\t' => s.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(s, "\\u{:04x}", c as u32);
            }
            c => s.push(c),
        }
    }
    s.push('"');
    s
}

fn jnums(v: &[u32]) -> String {
    format!("[{}]", v.iter().map(u32::to_string).collect::<Vec<_>>().join(","))
}

fn jstrs(v: &[String]) -> String {
    format!("[{}]", v.iter().map(|x| jstr(x)).collect::<Vec<_>>().join(","))
}

fn common(p: &Plan, n: u32) -> String {
    let after: Vec<u32> = p.stacked.get(&n).cloned().unwrap_or_default();
    let head = p.heads.get(&n).map(String::as_str).unwrap_or("");
    let title = p.titles.get(&n).map(String::as_str).unwrap_or("");
    format!("\"number\":{n},\"head\":{},\"title\":{},\"after\":{}", jstr(head), jstr(title), jnums(&after))
}

fn wave2_entry(p: &Plan, w: &Refresh) -> String {
    format!(
        "{{{},\"kind\":{},\"why\":{},\"files\":{}}}",
        common(p, w.number),
        jstr(w.kind.label()),
        jstr(&w.why),
        jstrs(&w.files)
    )
}

/// The plan as one JSON object: `base`, `wave1` (in merge order), `wave2` (in refresh order), `unknown`. Keys and
/// arrays are written in a fixed order, so equal plans give equal bytes.
pub fn to_json(p: &Plan) -> String {
    let w1: Vec<String> = p.wave1.iter().map(|n| format!("{{{},\"kind\":\"clean\"}}", common(p, *n))).collect();
    let w2: Vec<String> = p.wave2.iter().map(|w| wave2_entry(p, w)).collect();
    format!(
        "{{\"base\":{},\"wave1\":[{}],\"wave2\":[{}],\"unknown\":{}}}\n",
        jstr(&p.base_sha),
        w1.join(","),
        w2.join(","),
        jnums(&p.unknown)
    )
}
