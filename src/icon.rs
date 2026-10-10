//! mimamori's icon, drawn in code: four core tiles on the theme's dark panel,
//! each filled from the bottom to a load in the colour the usage ramp gives
//! it, the way the cpu screen draws a core.
//!
//! Drawn rather than kept as a picture so that the window's icon (set at
//! startup) and the one `build.rs` bakes into `mimamori.exe` are the same at
//! every size, with no image decoder and no file to keep in step. `build.rs`
//! includes this file as it is, so it uses nothing but `std`.

/// The panel and its edge: tsumugi Dark's `bg` and `border`.
const BG: [u8; 3] = [0x16, 0x18, 0x1d];
const EDGE: [u8; 3] = [0x2c, 0x30, 0x39];
/// A tile's empty part: `theme::bar_of` in tsumugi Dark.
const EMPTY: [u8; 3] = [0x20, 0x23, 0x2a];

/// The tiles, left to right and top to bottom: the load each is filled to
/// and its colour on the ramp (cyan, blue, yellow, cyan).
const TILES: [(f32, [u8; 3]); 4] =
    [(0.55, [0x6f, 0xd0, 0xd0]), (0.25, [0x7a, 0xb8, 0xf5]), (0.80, [0xe8, 0xc8, 0x7a]), (0.45, [0x6f, 0xd0, 0xd0])];

/// Samples per pixel along each side, for the edges' anti-aliasing.
const SS: u32 = 4;

/// The icon `px` pixels square, as straight (not premultiplied) RGBA, row by
/// row from the top.
pub fn rgba(px: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((px * px * 4) as usize);
    let n = (SS * SS) as f32;
    for y in 0..px {
        for x in 0..px {
            let mut sum = [0.0f32; 3];
            let mut alpha = 0.0f32;
            for sy in 0..SS {
                for sx in 0..SS {
                    let u = (x as f32 + (sx as f32 + 0.5) / SS as f32) / px as f32;
                    let v = (y as f32 + (sy as f32 + 0.5) / SS as f32) / px as f32;
                    if let Some(c) = sample(u, v) {
                        for (s, c) in sum.iter_mut().zip(c) {
                            *s += c as f32;
                        }
                        alpha += 1.0;
                    }
                }
            }
            if alpha == 0.0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                for s in sum {
                    out.push((s / alpha).round() as u8);
                }
                out.push((alpha / n * 255.0).round() as u8);
            }
        }
    }
    out
}

/// The colour at (`u`, `v`) of the unit square, or `None` outside the panel.
fn sample(u: f32, v: f32) -> Option<[u8; 3]> {
    let panel = rounded(u, v, (0.5, 0.5), 0.47, 0.18);
    if panel >= 0.0 {
        return None;
    }
    let mut c = if panel > -0.035 { EDGE } else { BG };
    // Two by two in 0.17..0.83, 0.06 apart.
    let (lo, size, gap) = (0.17, 0.30, 0.06);
    for (i, &(load, fill)) in TILES.iter().enumerate() {
        let left = lo + (i % 2) as f32 * (size + gap);
        let top = lo + (i / 2) as f32 * (size + gap);
        let half = size / 2.0;
        if rounded(u, v, (left + half, top + half), half, 0.05) < 0.0 {
            c = if v > top + size * (1.0 - load) { fill } else { EMPTY };
        }
    }
    Some(c)
}

/// The signed distance from (`u`, `v`) to a square around `c` of half side
/// `half` with corners of radius `r`: negative inside.
fn rounded(u: f32, v: f32, c: (f32, f32), half: f32, r: f32) -> f32 {
    let qx = (u - c.0).abs() - half + r;
    let qy = (v - c.1).abs() - half + r;
    let out = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    out + qx.max(qy).min(0.0) - r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::theme;

    fn at(img: &[u8], px: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * px + x) * 4) as usize;
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }

    fn rgb(c: egui::Color32) -> [u8; 3] {
        [c.r(), c.g(), c.b()]
    }

    #[test]
    fn the_colours_are_the_themes() {
        let dark = ito_theme::builtin()[0].colors;
        assert_eq!(BG, rgb(dark.bg));
        assert_eq!(EDGE, rgb(dark.border));
        assert_eq!(EMPTY, rgb(theme::bar_of(&dark)));
        let ramp = [(0.0, dark.blue), (0.3, dark.run), (0.7, dark.wait), (0.9, dark.err)];
        for (load, fill) in TILES {
            let want = ramp.iter().rev().find(|(at, _)| load >= *at).unwrap().1;
            assert_eq!(fill, rgb(want), "{load}");
        }
    }

    #[test]
    fn the_corners_are_clear_and_the_tiles_filled_from_the_bottom() {
        let px = 64;
        let img = rgba(px);
        assert_eq!(img.len(), (px * px * 4) as usize);
        assert_eq!(at(&img, px, 0, 0)[3], 0);
        assert_eq!(at(&img, px, px - 1, px - 1)[3], 0);
        // The gap between the tiles, in the middle, is the panel.
        assert_eq!(at(&img, px, 32, 32), [BG[0], BG[1], BG[2], 255]);
        // The yellow tile (bottom left, 0.53..0.83) is full near its foot and
        // empty near its head.
        let x = (0.32 * px as f32) as u32;
        let [r, g, b, _] = at(&img, px, x, (0.80 * px as f32) as u32);
        assert_eq!([r, g, b], TILES[2].1);
        let [r, g, b, _] = at(&img, px, x, (0.555 * px as f32) as u32);
        assert_eq!([r, g, b], EMPTY);
    }

    #[test]
    fn every_size_draws() {
        for px in [16, 24, 32, 48, 256] {
            let img = rgba(px);
            assert_eq!(img.len(), (px * px * 4) as usize);
            assert!(img.chunks(4).any(|p| p[3] == 255), "{px}");
        }
    }
}
