//! Pins the logotype pipeline (#1459): the committed icon sizes are in step with the source, the ICO
//! container is well formed, and a non-square source is padded, not stretched.
//! Provenance: our own pipeline; ICO layout from the public Windows icon-file format.

use crate::logo;
use resvg::tiny_skia::Pixmap;

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn committed_logo_sizes_match_the_source() -> Result<(), String> {
    for (p, b) in logo::generate(&root())? {
        assert_eq!(std::fs::read(&p).ok().as_deref(), Some(b.as_slice()), "{} is stale: cargo xtask logo", p.display());
    }
    Ok(())
}

#[test]
fn ico_header_and_offsets_are_consistent() -> Result<(), String> {
    let ico = logo::ico(&[(16, vec![1, 2, 3]), (256, vec![4, 5])])?;
    assert_eq!(&ico[..6], &[0, 0, 1, 0, 2, 0]);
    assert_eq!(ico[6], 16);
    assert_eq!(ico[22], 0, "256 is stored as 0");
    assert_eq!(u32::from_le_bytes([ico[18], ico[19], ico[20], ico[21]]), 38);
    assert_eq!(&ico[38..], &[1, 2, 3, 4, 5]);
    Ok(())
}

#[test]
fn a_wide_source_is_padded_to_a_square() -> Result<(), String> {
    let mut src = Pixmap::new(4, 2).ok_or("pixmap")?;
    for p in src.data_mut().as_chunks_mut::<4>().0 {
        p.copy_from_slice(&[10, 20, 30, 255]);
    }
    let out = logo::resize_square(&src, 2)?;
    assert_eq!((out.width(), out.height()), (2, 2));
    assert!(out.data().as_chunks::<4>().0.iter().all(|p| *p == [10, 20, 30, 255]));
    Ok(())
}
