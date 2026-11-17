//! Rewrites the prepared HUD movie for a look: HUD color, warning color, opacity.
//!
//! tools/prepare_hud.py prepares the movie at build time: it appends a table of where every color is (shape fills
//! and outlines, text colors, color transforms) and adds a neutral color transform to each top-level element. So
//! nothing here reads the file format: each table entry's color goes through one rule (Recolor::rule) and is
//! written back in place, and the opacity is written into the added transforms' alpha.

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Look {
    /// 0xRRGGBB.
    pub hud: u32,
    pub warning: u32,
    /// Percent.
    pub opacity: u32,
}

impl Look {
    /// The movie's own base color and red: these leave its colors alone.
    pub(crate) const STOCK: Look = Look { hud: 0xDFFCFF, warning: 0xFF0000, opacity: 100 };
}

// The table's footer, at the end of the file: entry count, offset of the table (= the movie's length), this.
const MAGIC: &[u8; 8] = b"PMHUDTBL";
const FOOTER_SIZE: usize = 16;
const ENTRY_SIZE: usize = 8;

/// A table entry: where a color is (bit offset) and how it is stored.
struct Entry {
    bit: usize,
    kind: u8,
    /// Of each term, in bits (color transforms).
    width: usize,
    /// The grey multiply of a mix (256 = 1.0).
    multiply: i16,
}

// Red, green, blue bytes (fills, outlines, text).
const KIND_COLOR_BYTES: u8 = 0;
// A color transform's multiply terms (256 = 1.0): the multiply is the color.
const KIND_TINT: u8 = 1;
// A color transform's add terms: a mix of what's below (the multiply's share) and a color, add = color * the rest.
const KIND_MIX: u8 = 2;
// The alpha multiply of a transform the preparation added to a top-level element.
const KIND_ALPHA: u8 = 3;

/// The prepared file's movie and table entries. None if it isn't a prepared file.
fn parts(prepared: &[u8]) -> Option<(&[u8], impl Iterator<Item = Entry> + '_)> {
    let footer = prepared.get(prepared.len().checked_sub(FOOTER_SIZE)?..)?;
    if &footer[8..] != MAGIC {
        return None;
    }
    let count = u32::from_le_bytes(footer[..4].try_into().ok()?) as usize;
    let movie_length = u32::from_le_bytes(footer[4..8].try_into().ok()?) as usize;
    let table = prepared.get(movie_length..movie_length + count * ENTRY_SIZE)?;
    let entries = table.chunks_exact(ENTRY_SIZE).map(|entry| Entry {
        bit: u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]) as usize,
        kind: entry[4],
        width: entry[5] as usize,
        multiply: i16::from_le_bytes([entry[6], entry[7]]),
    });
    Some((&prepared[..movie_length], entries))
}

pub(crate) fn is_prepared(file: &[u8]) -> bool {
    parts(file).is_some()
}

/// The movie with the look applied, and how many colors were rewritten. None if the file isn't a prepared one.
pub(crate) fn with_look(prepared: &[u8], look: Look) -> Option<(Vec<u8>, usize)> {
    let (movie, entries) = parts(prepared)?;
    let mut recolor = Recolor::new(movie.to_vec(), look);
    let recolors = (look.hud, look.warning) != (Look::STOCK.hud, Look::STOCK.warning);
    for entry in entries {
        match entry.kind {
            KIND_ALPHA => recolor.set_signed_bits(entry.bit, entry.width, (look.opacity * 256 / 100) as i64),
            _ if !recolors => None,
            KIND_COLOR_BYTES => recolor.color_bytes(entry.bit / 8),
            KIND_TINT => recolor.tint(&entry),
            KIND_MIX => recolor.mix(&entry),
            _ => None,
        };
    }
    Some((recolor.movie, recolor.colors))
}

// A color under this saturation is one of the HUD's base colors (pale cyan, white, greys).
const PALE_BELOW: f64 = 0.35;

fn rgb_to_hsv([r, g, b]: [f64; 3]) -> [f64; 3] {
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    if max == min {
        return [0.0, 0.0, max];
    }
    let range = max - min;
    let (rc, gc, bc) = ((max - r) / range, (max - g) / range, (max - b) / range);
    let hue = if r == max {
        bc - gc
    } else if g == max {
        2.0 + rc - bc
    } else {
        4.0 + gc - rc
    };
    [(hue / 6.0).rem_euclid(1.0), range / max, max]
}

fn hsv_to_rgb([h, s, v]: [f64; 3]) -> [f64; 3] {
    if s == 0.0 {
        return [v, v, v];
    }
    let sector = (h * 6.0) as i64;
    let f = h * 6.0 - sector as f64;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match sector.rem_euclid(6) {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

fn channels(rgb: u32) -> [f64; 3] {
    [rgb >> 16 & 0xFF, rgb >> 8 & 0xFF, rgb & 0xFF].map(|c| c as f64 / 255.0)
}

fn half_up(value: f64) -> i64 {
    (value + 0.5) as i64
}

struct Recolor {
    movie: Vec<u8>,
    hud: [f64; 3],
    // What takes pure red to the warning color: a hue rotation, and scales for saturation and brightness.
    warning_hue: f64,
    warning_saturation: f64,
    warning_brightness: f64,
    /// Colors rewritten so far.
    colors: usize,
}

impl Recolor {
    fn new(movie: Vec<u8>, look: Look) -> Self {
        let [warning_hue, warning_saturation, warning_brightness] = rgb_to_hsv(channels(look.warning));
        Recolor { movie, hud: channels(look.hud), warning_hue, warning_saturation, warning_brightness, colors: 0 }
    }

    /// The rule, for one color (channels 0..1): a pale or grey color becomes the HUD color at its brightness; a
    /// saturated one is rotated and scaled like red is to become the warning color.
    fn rule(&mut self, color: [f64; 3]) -> [f64; 3] {
        self.colors += 1;
        let [h, s, v] = rgb_to_hsv(color.map(|c| c.clamp(0.0, 1.0)));
        if v == 0.0 {
            [0.0; 3]
        } else if s < PALE_BELOW {
            self.hud.map(|c| c * v)
        } else {
            hsv_to_rgb([(h + self.warning_hue).rem_euclid(1.0), s * self.warning_saturation, v * self.warning_brightness])
        }
    }

    /// Reads a signed field of `count` bits (most significant first) at bit `pos`.
    fn signed_bits(&self, pos: usize, count: usize) -> Option<i64> {
        let mut value: i64 = 0;
        for i in pos..pos + count {
            value = value << 1 | (*self.movie.get(i / 8)? >> (7 - i % 8) & 1) as i64;
        }
        Some(if count > 0 && value >> (count - 1) != 0 { value - (1 << count) } else { value })
    }

    /// Writes one; a value too large for the field becomes the largest that fits.
    fn set_signed_bits(&mut self, pos: usize, count: usize, value: i64) -> Option<()> {
        let value = value.min((1 << count.checked_sub(1)?) - 1);
        for i in 0..count {
            let bit = (value >> (count - 1 - i) & 1) as u8;
            let shift = 7 - (pos + i) % 8;
            let byte = self.movie.get_mut((pos + i) / 8)?;
            *byte = *byte & !(1 << shift) | bit << shift;
        }
        Some(())
    }

    /// The red, green and blue terms of a color transform.
    fn terms(&self, entry: &Entry) -> Option<[i64; 3]> {
        let mut values = [0; 3];
        for (i, value) in values.iter_mut().enumerate() {
            *value = self.signed_bits(entry.bit + i * entry.width, entry.width)?;
        }
        Some(values)
    }

    fn set_terms(&mut self, entry: &Entry, values: [i64; 3]) -> Option<()> {
        for (i, value) in values.into_iter().enumerate() {
            self.set_signed_bits(entry.bit + i * entry.width, entry.width, value)?;
        }
        Some(())
    }

    fn color_bytes(&mut self, pos: usize) -> Option<()> {
        let color: [u8; 3] = self.movie.get(pos..pos + 3)?.try_into().ok()?;
        let new = self.rule(color.map(|c| c as f64 / 255.0));
        self.movie[pos..pos + 3].copy_from_slice(&new.map(|c| half_up(c * 255.0).min(255) as u8));
        Some(())
    }

    fn tint(&mut self, entry: &Entry) -> Option<()> {
        let multiply = self.terms(entry)?;
        let new = self.rule(multiply.map(|m| m as f64 / 256.0));
        self.set_terms(entry, new.map(|c| half_up(c * 256.0)))
    }

    /// Multiply 0 sets the color; multiply 1 only adds, and the color is the add terms at full brightness.
    fn mix(&mut self, entry: &Entry) -> Option<()> {
        let add = self.terms(entry)?;
        let mut share = 1.0 - entry.multiply as f64 / 256.0;
        if share <= 0.0 {
            share = *add.iter().max()? as f64 / 255.0;
        }
        let new = self.rule(add.map(|a| a as f64 / 255.0 / share));
        self.set_terms(entry, new.map(|c| half_up(c * 255.0 * share)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against files made by an independent implementation of the rule (hud-color/tools/movie-colors/
    /// apply_table.py): set HUD_PREPARED to the prepared movie and HUD_REFERENCES to a folder of
    /// ref_<HUD RRGGBB>_<warning RRGGBB>_<opacity>.gfx files. Skipped without them.
    #[test]
    fn matches_references() {
        let (Ok(prepared), Ok(folder)) = (std::env::var("HUD_PREPARED"), std::env::var("HUD_REFERENCES")) else { return };
        let prepared = std::fs::read(prepared).unwrap();
        let mut checked = 0;
        for entry in std::fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let Some(values) = name.strip_prefix("ref_").and_then(|n| n.strip_suffix(".gfx")) else { continue };
            let values: Vec<&str> = values.split('_').collect();
            let [hud, warning, opacity] = values[..] else { continue };
            let look = Look {
                hud: u32::from_str_radix(hud, 16).unwrap(),
                warning: u32::from_str_radix(warning, 16).unwrap(),
                opacity: opacity.parse().unwrap(),
            };
            let (ours, _) = with_look(&prepared, look).unwrap();
            assert!(ours == std::fs::read(&path).unwrap(), "{name}");
            checked += 1;
        }
        println!("{checked} references match");
    }
}
