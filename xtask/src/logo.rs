//! `cargo xtask logo [--check]` (#1459): turns the maintainer's logotype (`assets/logo/camcraft-logo.png`,
//! his own artwork, used as given) into the sizes the app and the packaging load: square PNGs under
//! `assets/logo/icons/` and a Windows `camcraft.ico` (PNG-compressed entries, ICO container written here).
//! The source is padded to a square with its own corner colour, then area-averaged down (no new
//! dependency: resvg's tiny-skia decodes and encodes the PNGs). Provenance: the ICO layout is the
//! public Windows icon-file format; the resampling is a plain box filter.

use std::path::{Path, PathBuf};

use resvg::tiny_skia::Pixmap;

/// Edges of the generated PNGs (window icon, Linux hicolor sizes, macOS iconset base sizes).
pub const SIZES: [u32; 7] = [16, 32, 48, 64, 128, 256, 512];
/// Edges packed into the `.ico` (the sizes Windows asks for).
pub const ICO_SIZES: [u32; 4] = [16, 32, 48, 256];

pub fn source(root: &Path) -> PathBuf {
    root.join("assets").join("logo").join("camcraft-logo.png")
}

pub fn out_dir(root: &Path) -> PathBuf {
    root.join("assets").join("logo").join("icons")
}

/// Pad to a square (corner colour, centred), then box-filter to `px` x `px`. Opaque RGBA out.
pub fn resize_square(src: &Pixmap, px: u32) -> Result<Pixmap, String> {
    let (w, h) = (src.width() as usize, src.height() as usize);
    if w == 0 || h == 0 || px == 0 {
        return Err("empty image or size".into());
    }
    let s = w.max(h);
    let bg: [u8; 4] = src.data().get(0..4).and_then(|c| c.try_into().ok()).ok_or("short pixel data")?;
    let (ox, oy) = ((s - w) / 2, (s - h) / 2);
    let at = |x: usize, y: usize| -> [u8; 4] {
        if x >= ox && x < ox + w && y >= oy && y < oy + h {
            let o = ((y - oy) * w + (x - ox)) * 4;
            src.data().get(o..o + 4).and_then(|c| c.try_into().ok()).unwrap_or(bg)
        } else {
            bg
        }
    };
    let mut out = Pixmap::new(px, px).ok_or("cannot allocate the icon")?;
    let n = px as usize;
    for (i, d) in out.data_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (dx, dy) = (i % n, i / n);
        let (x0, x1) = (dx * s / n, ((dx + 1) * s / n).max(dx * s / n + 1));
        let (y0, y1) = (dy * s / n, ((dy + 1) * s / n).max(dy * s / n + 1));
        let mut acc = [0u64; 4];
        for y in y0..y1 {
            for x in x0..x1 {
                for (a, v) in acc.iter_mut().zip(at(x, y)) {
                    *a += u64::from(v);
                }
            }
        }
        let cnt = ((x1 - x0) * (y1 - y0)) as u64;
        for (o, a) in d.iter_mut().zip(acc) {
            *o = (a / cnt) as u8;
        }
    }
    Ok(out)
}

/// An ICO file holding the given PNG-encoded square images (edge, bytes).
pub fn ico(entries: &[(u32, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let n = u16::try_from(entries.len()).map_err(|_| "too many ico entries")?;
    let mut out = vec![0, 0, 1, 0];
    out.extend(n.to_le_bytes());
    let mut offset = 6 + 16 * entries.len();
    for (edge, png) in entries {
        let b = if *edge >= 256 { 0 } else { *edge as u8 };
        out.extend([b, b, 0, 0, 1, 0, 32, 0]);
        out.extend((png.len() as u32).to_le_bytes());
        out.extend((offset as u32).to_le_bytes());
        offset += png.len();
    }
    for (_, png) in entries {
        out.extend(png);
    }
    Ok(out)
}

/// Every generated file as (path, bytes), from the source.
pub fn generate(root: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>, String> {
    let sp = source(root);
    let bytes = std::fs::read(&sp).map_err(|e| format!("{}: {e}", sp.display()))?;
    let src = Pixmap::decode_png(&bytes).map_err(|e| format!("{}: {e}", sp.display()))?;
    let dir = out_dir(root);
    let mut files = Vec::new();
    let mut ico_in = Vec::new();
    for px in SIZES {
        let png = resize_square(&src, px)?.encode_png().map_err(|e| e.to_string())?;
        if ICO_SIZES.contains(&px) {
            ico_in.push((px, png.clone()));
        }
        files.push((dir.join(format!("camcraft-{px}.png")), png));
    }
    files.push((dir.join("camcraft.ico"), ico(&ico_in)?));
    Ok(files)
}

/// `cargo xtask logo [--check]`: write the sizes, or report files missing or out of step with the source.
pub fn run(root: &Path, args: &[String]) -> Result<i32, String> {
    let files = generate(root)?;
    match args.first().map(String::as_str) {
        None => {
            std::fs::create_dir_all(out_dir(root)).map_err(|e| e.to_string())?;
            for (p, b) in &files {
                std::fs::write(p, b).map_err(|e| format!("{}: {e}", p.display()))?;
            }
            println!("logo: wrote {} file(s) under assets/logo/icons/", files.len());
            Ok(0)
        }
        Some("--check") => {
            let bad: Vec<_> = files.iter().filter(|(p, b)| std::fs::read(p).ok().as_deref() != Some(b.as_slice())).collect();
            for (p, _) in &bad {
                println!("{}: missing or out of step - run `cargo xtask logo`", p.display());
            }
            println!("logo: {} breach(es)", bad.len());
            Ok(i32::from(!bad.is_empty()))
        }
        Some(o) => Err(format!("unknown logo option '{o}' (none | --check)")),
    }
}
