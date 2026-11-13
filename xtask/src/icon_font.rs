//! A 3x5 pixel bitmap font for the labels under each icon of `cargo xtask icons --sheet` (#661):
//! lower-case letters, digits and '-' - the alphabet of the masters' file stems. The sheet is a
//! review aid, not an icon, so the masters' shapes-only rule does not apply to it.
//! Provenance: our own glyph bitmaps, drawn for this sheet.

/// One glyph: five rows, top first; bit 2 is the left column, bit 0 the right.
fn rows(c: char) -> [u8; 5] {
    match c {
        'a' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'b' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'c' => [0b011, 0b100, 0b100, 0b100, 0b011],
        'd' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'e' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'f' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'g' => [0b011, 0b100, 0b101, 0b101, 0b011],
        'h' => [0b101, 0b101, 0b111, 0b101, 0b101],
        'i' => [0b111, 0b010, 0b010, 0b010, 0b111],
        'j' => [0b001, 0b001, 0b001, 0b101, 0b010],
        'k' => [0b101, 0b101, 0b110, 0b101, 0b101],
        'l' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'm' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'n' => [0b110, 0b101, 0b101, 0b101, 0b101],
        'o' => [0b010, 0b101, 0b101, 0b101, 0b010],
        'p' => [0b110, 0b101, 0b110, 0b100, 0b100],
        'q' => [0b010, 0b101, 0b101, 0b110, 0b011],
        'r' => [0b110, 0b101, 0b110, 0b101, 0b101],
        's' => [0b011, 0b100, 0b010, 0b001, 0b110],
        't' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'u' => [0b101, 0b101, 0b101, 0b101, 0b111],
        'v' => [0b101, 0b101, 0b101, 0b101, 0b010],
        'w' => [0b101, 0b101, 0b111, 0b111, 0b101],
        'x' => [0b101, 0b101, 0b010, 0b101, 0b101],
        'y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        'z' => [0b111, 0b001, 0b010, 0b100, 0b111],
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b110, 0b001, 0b010, 0b100, 0b111],
        '3' => [0b110, 0b001, 0b010, 0b001, 0b110],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b110, 0b001, 0b110],
        '6' => [0b011, 0b100, 0b110, 0b101, 0b010],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b010, 0b101, 0b010, 0b101, 0b010],
        '9' => [0b010, 0b101, 0b011, 0b001, 0b110],
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        _ => [0b111, 0b101, 0b101, 0b101, 0b111],
    }
}

/// Pixel width of one character cell (3 columns plus a 1 px gap) and of a text line's height.
pub const ADVANCE: u32 = 4;
pub const LINE: u32 = 6;

/// The lit pixels of `text` laid out on one line from (0, 0): `(x, y)` pairs.
pub fn pixels(text: &str) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for (i, c) in text.chars().enumerate() {
        let x0 = i as u32 * ADVANCE;
        for (y, row) in rows(c.to_ascii_lowercase()).iter().enumerate() {
            for x in 0..3u32 {
                if row & (0b100 >> x) != 0 {
                    out.push((x0 + x, y as u32));
                }
            }
        }
    }
    out
}

/// A label broken into lines of at most `max` characters, at '-' where it can: the sheet's cells
/// are narrow and the stems are long (`tool-threading-insert`). The family prefix stays: `op-drill`
/// and `tool-drill` must not read alike on the sheet either.
pub fn label_lines(stem: &str, max: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in stem.split('-') {
        match lines.last_mut() {
            Some(last) if last.len() + 1 + word.len() <= max => {
                last.push('-');
                last.push_str(word);
            }
            _ => {
                let chars: Vec<char> = word.chars().collect();
                for chunk in chars.chunks(max.max(1)) {
                    lines.push(chunk.iter().collect());
                }
            }
        }
    }
    lines
}
