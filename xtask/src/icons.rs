//! `cargo xtask icons`: rasterises the SVG icon masters (`assets/ui/svg/*.svg`) to the 32 px PNGs the
//! UI loads (`assets/ui/<stem>.png`), checks the committed PNGs against their masters (`--check`), and
//! draws a 16 px contact sheet on a dark and a light band (`--sheet <out.png>`) for a human look.
//! Provenance: our own pipeline (#661, docs/decisions/661-icon-set.md); rendering by resvg
//! (MIT OR Apache-2.0); the masters keep Tabler Icons' conventions (24-unit grid, 2-unit stroke,
//! round caps and joins) so they sit with the Tabler glyphs we ship.

use std::path::{Path, PathBuf};

use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg;

use crate::icon_font;

/// The edge of every committed PNG (AGENTS.md "Assets": 32x32, drawn at 16 px).
pub const PNG_PX: u32 = 32;
/// The masters' grid (Tabler's `viewBox="0 0 24 24"`).
const GRID: f32 = 24.0;
/// Largest per-channel difference `--check` accepts between a fresh render and the committed PNG:
/// resvg's anti-aliasing may differ by a step or two across CPUs, a changed drawing differs by far more.
const TOLERANCE: u8 = 8;
/// The only stroke weights a master may use: Tabler's 2 for the object, 1.5 for a toolpath or
/// detail line drawn over it.
const WEIGHTS: [&str; 2] = ["1.5", "2"];

/// The masters directory and the PNG directory, relative to the workspace root.
pub fn dirs(root: &Path) -> (PathBuf, PathBuf) {
    let ui = root.join("assets").join("ui");
    (ui.join("svg"), ui)
}

/// Every `*.svg` master, sorted by name.
pub fn masters(root: &Path) -> Result<Vec<PathBuf>, String> {
    let (svg, _) = dirs(root);
    let rd = std::fs::read_dir(&svg).map_err(|e| format!("{}: {e}", svg.display()))?;
    let mut out: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "svg"))
        .collect();
    out.sort();
    Ok(out)
}

/// The house rules a master must keep; one line per breach (empty = clean). Shapes only: no raster,
/// no external reference, no script, no font or text, no stylesheet; the 24-unit grid with no
/// intrinsic size; round caps and joins; only the two stroke weights.
pub fn lint(text: &str) -> Vec<String> {
    let mut bad = Vec::new();
    let lower = text.to_ascii_lowercase();
    for banned in ["<image", "<script", "<text", "<foreignobject", "<style", "href", "url(", "font", "@import"] {
        if lower.contains(banned) {
            bad.push(format!("contains '{banned}' (shapes only)"));
        }
    }
    let root_tag = lower.split_once("<svg").map(|(_, r)| r.split('>').next().unwrap_or("")).unwrap_or("");
    if root_tag.is_empty() {
        bad.push("no <svg> root".into());
    }
    if !root_tag.contains("viewbox=\"0 0 24 24\"") {
        bad.push("root must carry viewBox=\"0 0 24 24\"".into());
    }
    if root_tag.contains(" width=") || root_tag.contains(" height=") {
        bad.push("root must carry no intrinsic width/height".into());
    }
    for cap in ["butt", "square", "miter", "bevel"] {
        if lower.contains(&format!("\"{cap}\"")) {
            bad.push(format!("'{cap}' cap/join (round only)"));
        }
    }
    if !root_tag.contains("stroke-linecap=\"round\"") || !root_tag.contains("stroke-linejoin=\"round\"") {
        bad.push("root must set stroke-linecap and stroke-linejoin to round".into());
    }
    let mut rest = lower.as_str();
    while let Some(i) = rest.find("stroke-width=\"") {
        let tail = &rest[i + 14..];
        let v = tail.split('"').next().unwrap_or("");
        if !WEIGHTS.contains(&v) {
            bad.push(format!("stroke-width {v} (allowed: {})", WEIGHTS.join(", ")));
        }
        rest = tail;
    }
    bad
}

/// Render one master at `px` x `px` (black ink with alpha coverage; the UI recolours it at load).
pub fn render(svg: &[u8], px: u32) -> Result<Pixmap, String> {
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).map_err(|e| e.to_string())?;
    let mut pm = Pixmap::new(px, px).ok_or_else(|| format!("cannot allocate a {px} px pixmap"))?;
    let s = px as f32 / GRID;
    resvg::render(&tree, Transform::from_scale(s, s), &mut pm.as_mut());
    Ok(pm)
}

fn stem(p: &Path) -> Result<String, String> {
    p.file_stem()
        .and_then(|s| s.to_str())
        .map(String::from)
        .ok_or_else(|| format!("{}: no file stem", p.display()))
}

/// One master's problems against its committed PNG and the house rules (empty = in step).
pub fn check_one(master: &Path, png_dir: &Path) -> Result<Vec<String>, String> {
    let name = stem(master)?;
    let bytes = std::fs::read(master).map_err(|e| format!("{}: {e}", master.display()))?;
    let mut bad: Vec<String> = lint(&String::from_utf8_lossy(&bytes)).into_iter().map(|b| format!("{name}.svg: {b}")).collect();
    let fresh = render(&bytes, PNG_PX).map_err(|e| format!("{name}.svg: {e}"))?;
    let png = png_dir.join(format!("{name}.png"));
    match std::fs::read(&png).map_err(|e| e.to_string()).and_then(|b| Pixmap::decode_png(&b).map_err(|e| e.to_string())) {
        Err(e) => bad.push(format!("{name}.png: missing or unreadable ({e}) - run `cargo xtask icons`")),
        Ok(old) if old.width() != PNG_PX || old.height() != PNG_PX => {
            bad.push(format!("{name}.png: {}x{}, want {PNG_PX}x{PNG_PX}", old.width(), old.height()));
        }
        Ok(old) => {
            let worst = old.data().iter().zip(fresh.data()).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
            if worst > TOLERANCE {
                bad.push(format!("{name}.png: differs from its master by {worst} - run `cargo xtask icons`"));
            }
        }
    }
    Ok(bad)
}

/// `--check` over every master: the breaches, one per line.
pub fn check(root: &Path) -> Result<Vec<String>, String> {
    let (_, png_dir) = dirs(root);
    let mut bad = Vec::new();
    for m in masters(root)? {
        bad.extend(check_one(&m, &png_dir)?);
    }
    Ok(bad)
}

/// Rasterise every master to its PNG; refuses (writes nothing for) a master that breaks the rules.
pub fn build(root: &Path) -> Result<usize, String> {
    let (_, png_dir) = dirs(root);
    let mut n = 0;
    for m in masters(root)? {
        let name = stem(&m)?;
        let bytes = std::fs::read(&m).map_err(|e| format!("{}: {e}", m.display()))?;
        let bad = lint(&String::from_utf8_lossy(&bytes));
        if !bad.is_empty() {
            return Err(format!("{name}.svg: {}", bad.join("; ")));
        }
        let pm = render(&bytes, PNG_PX).map_err(|e| format!("{name}.svg: {e}"))?;
        let png = pm.encode_png().map_err(|e| format!("{name}: {e}"))?;
        std::fs::write(png_dir.join(format!("{name}.png")), png).map_err(|e| format!("{name}.png: {e}"))?;
        n += 1;
    }
    Ok(n)
}

/// Characters per label line under a sheet cell, and the most lines a label takes.
const LABEL_CHARS: u32 = 10;
const LABEL_LINES: u32 = 3;

/// A contact sheet: every master at `px` on a dark band (light ink) above a light band (dark ink),
/// `cols` per row, each pixel blown up `zoom` times (nearest) so a reviewer sees what 16 px shows.
/// Each icon carries its file stem under it (a 3x5 bitmap font), so a review can name what it sees.
pub fn sheet(root: &Path, px: u32, cols: u32, zoom: u32) -> Result<Pixmap, String> {
    let all = masters(root)?;
    let cell_w = (px + px / 2).max(LABEL_CHARS * icon_font::ADVANCE + 4);
    let cell_h = px + px / 2 + LABEL_LINES * icon_font::LINE;
    let rows = (all.len() as u32).div_ceil(cols.max(1)).max(1);
    let (w, band) = (cols * cell_w, rows * cell_h);
    let mut base = Pixmap::new(w, band * 2).ok_or("sheet too large")?;
    let fill = |pm: &mut Pixmap, y0: u32, rgb: [u8; 3]| {
        for (i, p) in pm.data_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
            if (i as u32 / w) >= y0 && (i as u32 / w) < y0 + band {
                p.copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
    };
    let bands: [(u32, [u8; 3], [u8; 3]); 2] = [(0, [30, 30, 32], [235, 235, 235]), (band, [246, 246, 246], [32, 36, 44])];
    for (y0, bg, _) in bands {
        fill(&mut base, y0, bg);
    }
    for (k, m) in all.iter().enumerate() {
        let bytes = std::fs::read(m).map_err(|e| format!("{}: {e}", m.display()))?;
        let icon = render(&bytes, px)?;
        let (x0, y0c) = ((k as u32 % cols) * cell_w, (k as u32 / cols) * cell_h);
        let (cx, cy) = (x0 + (cell_w - px) / 2, y0c + px / 4);
        let lines = icon_font::label_lines(&stem(m)?, LABEL_CHARS as usize);
        for (y0, _, ink) in bands {
            for (li, line) in lines.iter().take(LABEL_LINES as usize).enumerate() {
                let lx = x0 + (cell_w.saturating_sub(line.len() as u32 * icon_font::ADVANCE)) / 2;
                let ly = y0 + y0c + px + px / 4 + 2 + li as u32 * icon_font::LINE;
                for (dx, dy) in icon_font::pixels(line) {
                    let o = (((ly + dy) * w + lx + dx) * 4) as usize;
                    if let Some(d) = base.data_mut().get_mut(o..o + 3) {
                        d.copy_from_slice(&ink);
                    }
                }
            }
            for (j, p) in icon.data().as_chunks::<4>().0.iter().enumerate() {
                let a = u32::from(p[3]);
                let (x, y) = (cx + j as u32 % px, y0 + cy + j as u32 / px);
                let o = ((y * w + x) * 4) as usize;
                if let Some(d) = base.data_mut().get_mut(o..o + 3) {
                    for (c, inkc) in d.iter_mut().zip(ink) {
                        *c = ((u32::from(*c) * (255 - a) + u32::from(inkc) * a) / 255) as u8;
                    }
                }
            }
        }
    }
    let z = zoom.max(1);
    let mut big = Pixmap::new(w * z, band * 2 * z).ok_or("sheet too large")?;
    let src = base.data().to_vec();
    for (i, p) in big.data_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = ((i as u32 % (w * z)) / z, (i as u32 / (w * z)) / z);
        let o = ((y * w + x) * 4) as usize;
        if let Some(s) = src.get(o..o + 4) {
            p.copy_from_slice(s);
        }
    }
    Ok(big)
}

/// `cargo xtask icons [--check | --sheet <out.png> [--px N] [--zoom N]]`.
pub fn run(root: &Path, args: &[String]) -> Result<i32, String> {
    match args.first().map(String::as_str) {
        None => {
            println!("icons: wrote {} PNG(s) from assets/ui/svg/", build(root)?);
            Ok(0)
        }
        Some("--check") => {
            let bad = check(root)?;
            for b in &bad {
                println!("{b}");
            }
            println!("icons: {} breach(es)", bad.len());
            Ok(i32::from(!bad.is_empty()))
        }
        Some("--sheet") => {
            let out = args.get(1).ok_or("--sheet needs an output path")?;
            let num = |flag: &str, d: u32| -> Result<u32, String> {
                match args.iter().position(|a| a == flag) {
                    Some(i) => args.get(i + 1).and_then(|v| v.parse().ok()).ok_or(format!("{flag} needs a number")),
                    None => Ok(d),
                }
            };
            let pm = sheet(root, num("--px", 16)?, 8, num("--zoom", 4)?)?;
            pm.save_png(out).map_err(|e| format!("{out}: {e}"))?;
            for (k, m) in masters(root)?.iter().enumerate() {
                println!("{k:>3} {}", stem(m)?);
            }
            Ok(0)
        }
        Some(o) => Err(format!("unknown icons option '{o}' (none | --check | --sheet <out.png>)")),
    }
}
