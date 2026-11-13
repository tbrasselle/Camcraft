//! Pins the icon pipeline (#661): the masters' house rules, the render, and that every committed
//! PNG under assets/ui/ is in step with its SVG master (a master edited without `cargo xtask icons`
//! fails here, not in the running app).
//! Provenance: our own pipeline (docs/decisions/661-icon-set.md).

use crate::icons::{self, PNG_PX};

const CLEAN: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="2"><path d="M4 12H20"/></svg>"#;

#[test]
fn lint_accepts_a_clean_master() {
    assert_eq!(icons::lint(CLEAN), Vec::<String>::new());
}

#[test]
fn lint_refuses_raster_scripts_text_and_external_refs() {
    for bad in [
        r#"<image href="x.png"/>"#,
        "<script>alert(1)</script>",
        "<text>G0</text>",
        r#"<use href="other.svg#a"/>"#,
        r#"<rect fill="url(#g)"/>"#,
        "<style>path{}</style>",
    ] {
        let svg = CLEAN.replace("</svg>", &format!("{bad}</svg>"));
        assert!(!icons::lint(&svg).is_empty(), "lint let through {bad}");
    }
}

#[test]
fn lint_refuses_off_grid_sizes_caps_and_weights() {
    let off_grid = CLEAN.replace("0 0 24 24", "0 0 64 64");
    let sized = CLEAN.replace("<svg ", r#"<svg width="32" "#);
    let butt = CLEAN.replace(r#"stroke-linecap="round""#, r#"stroke-linecap="butt""#);
    let weight = CLEAN.replace(r#"stroke-width="2""#, r#"stroke-width="3""#);
    for svg in [off_grid, sized, butt, weight] {
        assert!(!icons::lint(&svg).is_empty(), "lint let through {svg}");
    }
}

#[test]
fn render_draws_ink_on_the_32_px_grid() {
    let pm = icons::render(CLEAN.as_bytes(), PNG_PX).expect("clean master renders");
    assert_eq!((pm.width(), pm.height()), (PNG_PX, PNG_PX));
    // The horizontal line through the centre is inked; a corner is not.
    let alpha = |x: u32, y: u32| pm.pixel(x, y).map(|p| p.alpha()).unwrap_or(0);
    assert!(alpha(16, 16) > 200, "centre of the stroke is inked");
    assert_eq!(alpha(1, 1), 0, "corner stays transparent");
}

#[test]
fn committed_icon_pngs_match_their_svg_masters() {
    let root = crate::root();
    let masters = icons::masters(&root).expect("assets/ui/svg/ readable");
    assert!(masters.len() >= 41, "the operation masters are present ({} found)", masters.len());
    let bad = icons::check(&root).expect("check runs");
    assert!(bad.is_empty(), "icon PNGs out of step - run `cargo xtask icons`:\n{}", bad.join("\n"));
}

/// The contact sheet names each icon (#661 slice 2): stems wrap at '-' into lines the narrow cell
/// holds, keeping the family prefix (`op-drill` and `tool-drill` must not read alike), and every
/// character of the stems' alphabet draws something.
#[test]
fn sheet_labels_wrap_stems_and_keep_the_family() {
    use crate::icon_font::{label_lines, pixels};
    assert_eq!(label_lines("tool-threading-insert", 10), ["tool", "threading", "insert"]);
    assert_eq!(label_lines("op-drill", 10), ["op-drill"]);
    assert_eq!(label_lines("tool-drill", 10), ["tool-drill"]);
    assert_eq!(label_lines("op-trochoidal-slot", 10), ["op", "trochoidal", "slot"]);
    for c in "abcdefghijklmnopqrstuvwxyz0123456789-".chars() {
        assert!(!pixels(&c.to_string()).is_empty(), "'{c}' draws nothing");
    }
    assert_ne!(pixels("o"), pixels("0"), "letter o and digit 0 differ");
}
