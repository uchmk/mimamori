//! The core tiles: one per physical core, grouped by L3 cache (and core
//! kind), with a bar per SMT thread inside. Compact on the overview, full on
//! the CPU screen.

use egui::{Align2, Id, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use super::{Cx, FONT};
use crate::collect::topo::Topology;
use crate::config::theme::{self, ramp};
use crate::i18n::trf;
use crate::screen::Pane;
use crate::util::fmt_bytes;

const TILE_GAP: f32 = 6.0;
const BAR_W: f32 = 10.0;
const BAR_GAP: f32 = 4.0;
/// The line over each group of tiles.
const GROUP_HEAD: f32 = 18.0;

/// A tile's width: room for a bar per thread.
fn tile_w(threads: usize, full: bool) -> f32 {
    let n = threads.max(1) as f32;
    (n * (BAR_W + BAR_GAP) + 16.0).max(if full { 64.0 } else { 56.0 })
}

fn tile_h(full: bool) -> f32 {
    if full { 92.0 } else { 46.0 }
}

/// A tile is never more than this many times as wide as it is tall, or the
/// other way round.
const STRETCH: f32 = 2.0;

/// How the tiles sit in their room: up to `cols` in a row, each `w` × `h`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub cols: usize,
    pub w: f32,
    pub h: f32,
}

impl Fit {
    /// The height of groups of `groups` cores laid out this way.
    fn height(&self, groups: &[usize]) -> f32 {
        let rows: usize = groups.iter().map(|n| n.max(&1).div_ceil(self.cols)).sum();
        let g = groups.len() as f32;
        g * GROUP_HEAD + rows as f32 * self.h + (rows as f32 - g) * TILE_GAP + (g - 1.0).max(0.0) * (TILE_GAP + 4.0)
    }
}

/// Full tiles grown to fill `width` × `height`: the number of columns that
/// gives the biggest tiles, each at least `min_w` × `min_h`. Groups (`groups`
/// holds their sizes) start on their own rows, so more columns than the
/// biggest group never help. `None` when even the smallest tiles do not fit.
pub fn fit(groups: &[usize], width: f32, height: f32, min_w: f32, min_h: f32) -> Option<Fit> {
    let most = groups.iter().copied().max()?.max(1);
    let g = groups.len() as f32;
    (1..=most)
        .filter_map(|cols| {
            let rows: usize = groups.iter().map(|n| n.max(&1).div_ceil(cols)).sum();
            let w = (width - (cols as f32 - 1.0) * TILE_GAP) / cols as f32;
            let room = height - g * GROUP_HEAD - (rows as f32 - g) * TILE_GAP - (g - 1.0).max(0.0) * (TILE_GAP + 4.0);
            let h = room / rows as f32;
            (w >= min_w && h >= min_h).then_some(Fit { cols, w: w.min(h * STRETCH), h: h.min(w * STRETCH) })
        })
        .max_by(|a, b| a.w.min(a.h).total_cmp(&b.w.min(b.h)))
}

/// Compact tiles at their own size, wrapped at `width`.
fn compact(topo: &Topology, width: f32) -> Fit {
    let w = tile_w(topo.cores().map(|c| c.threads.len()).max().unwrap_or(1), false);
    Fit { cols: (((width + TILE_GAP) / (w + TILE_GAP)).floor() as usize).max(1), w, h: tile_h(false) }
}

/// Lay the tiles out in `r`: full tiles grown to fill it when they fit,
/// compact ones wrapped otherwise. `sel` is the selected core (its place
/// across the chip) and `click` the column a clicked tile selects in, if
/// tiles are selectable here.
pub fn tiles(cx: &mut Cx, r: Rect, sel: Option<usize>, click: Option<Pane>) {
    let app = cx.app;
    let Some(st) = &app.model.st else { return };
    let usage = &app.model.usage;
    let freq = &app.model.freq;
    let topo = &st.topo;
    let sizes: Vec<usize> = topo.groups.iter().map(|g| g.cores.len()).collect();
    let most = topo.cores().map(|c| c.threads.len()).max().unwrap_or(1);
    let (lay, full) = match fit(&sizes, r.width(), r.height(), tile_w(most, true), tile_h(true)) {
        Some(f) => (f, true),
        None => (compact(topo, r.width()), false),
    };
    let (w, h) = (lay.w, lay.h);
    // Centred in the room when the tiles stop growing before they fill it.
    let block_w = lay.cols.min(sizes.iter().copied().max().unwrap_or(1)) as f32 * (w + TILE_GAP) - TILE_GAP;
    let left = if full { r.left() + ((r.width() - block_w) / 2.0).max(0.0) } else { r.left() };
    let right = left + block_w;
    let mut y = if full { r.top() + ((r.height() - lay.height(&sizes)) / 2.0).max(0.0) } else { r.top() };
    // Wider tiles get wider bars.
    let bar_w = if full { ((w - 16.0) / most.max(1) as f32 - BAR_GAP).clamp(BAR_W, 2.0 * BAR_W) } else { BAR_W };
    // Tiles are selected by their place in drawing order (the CPU screen's
    // rows), which is also `topo.cores()`'s order.
    let mut first = 0;
    for g in &topo.groups {
        let start = first;
        first += g.cores.len();
        if y + FONT > r.bottom() {
            break;
        }
        // The group's header: its label, its size, its cache.
        let mut head = trf("tiles.group", &[&g.label, &g.cores.len().to_string()]);
        if let Some(kb) = g.cache_kb {
            head += &format!(" · {}", trf("tiles.cache", &[&g.cache.to_string(), &fmt_bytes(kb * 1024)]));
        }
        cx.pen.text(pos2(left + 2.0, y + 8.0), &cx.pen.fit(&head, right - left), theme::dim());
        y += GROUP_HEAD;
        for (k, (ci, core)) in (start..).zip(&g.cores).enumerate() {
            let n = core.threads.len().max(1) as f32;
            if k > 0 && k % lay.cols == 0 {
                y += h + TILE_GAP;
            }
            if y + h > r.bottom() + 0.5 {
                break;
            }
            let x = left + (k % lay.cols) as f32 * (w + TILE_GAP);
            let t = Rect::from_min_size(pos2(x, y), vec2(w.min(r.width()), h));
            let vals: Vec<f32> = core.threads.iter().map(|&i| usage.get(i).copied().unwrap_or(0.0)).collect();
            let avg = if vals.is_empty() { 0.0 } else { vals.iter().sum::<f32>() / vals.len() as f32 };
            let selected = sel == Some(ci);
            cx.pen.p.rect_filled(t, 4.0, theme::bg());
            cx.pen.p.rect_stroke(t, 4.0, Stroke::new(1.0, if selected { theme::cyan() } else { theme::line() }), StrokeKind::Inside);

            // The core's name and its average.
            let top = t.top() + 9.0;
            cx.pen.small(
                pos2(t.left() + 5.0, top),
                Align2::LEFT_CENTER,
                &format!("C{}", core.id),
                if selected { theme::cyan() } else { theme::dim() },
            );
            cx.pen.small(pos2(t.right() - 5.0, top), Align2::RIGHT_CENTER, &format!("{avg:.0}%"), ramp(avg));

            // A bar per thread, centred.
            let bars_w = n * bar_w + (n - 1.0) * BAR_GAP;
            let bx = t.center().x - bars_w / 2.0;
            let (b_top, b_bot) = if full { (t.top() + 20.0, t.bottom() - 26.0) } else { (t.top() + 18.0, t.bottom() - 5.0) };
            for (k, (&cpu, &v)) in core.threads.iter().zip(&vals).enumerate() {
                let x0 = bx + k as f32 * (bar_w + BAR_GAP);
                let track = Rect::from_min_max(pos2(x0, b_top), pos2(x0 + bar_w, b_bot));
                cx.pen.p.rect_filled(track, 2.0, theme::line().gamma_multiply(0.6));
                let fh = track.height() * (v / 100.0).clamp(0.0, 1.0);
                if fh > 0.5 {
                    cx.pen.p.rect_filled(Rect::from_min_max(pos2(track.left(), track.bottom() - fh), track.max), 2.0, ramp(v));
                }
                if full {
                    cx.pen.small(pos2(track.center().x, b_bot + 7.0), Align2::CENTER_CENTER, &cpu.to_string(), theme::dim());
                }
            }
            if full {
                let mhz: Vec<u64> = core.threads.iter().filter_map(|&i| freq.get(i).copied()).filter(|&f| f > 0).collect();
                if !mhz.is_empty() {
                    let ghz = mhz.iter().sum::<u64>() as f32 / mhz.len() as f32 / 1000.0;
                    cx.pen.small(pos2(t.center().x, t.bottom() - 7.0), Align2::CENTER_CENTER, &format!("{ghz:.1}G"), theme::fg());
                }
            }
            if let Some(p) = click {
                let resp = cx.ui.interact(t, Id::new(("tile", app.screen, ci)), Sense::click());
                if resp.clicked() {
                    cx.out.click = Some((p, ci, resp.double_clicked()));
                }
            }
        }
        y += h + TILE_GAP + 4.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_tiles_grow_to_fill_the_room_in_balanced_rows() {
        // Two groups of eight in a wide, short room: a row each.
        let f = fit(&[8, 8], 1760.0, 560.0, 64.0, 92.0).unwrap();
        assert_eq!(f.cols, 8);
        assert!((f.w - (1760.0 - 7.0 * TILE_GAP) / 8.0).abs() < 0.01);
        assert!((f.height(&[8, 8]) - 560.0).abs() < 0.01);
        // Narrow and tall: fewer columns, more rows.
        let f = fit(&[8, 8], 420.0, 900.0, 64.0, 92.0).unwrap();
        assert_eq!(f.cols, 3);
        // Never stretched past twice as wide as tall: centred instead.
        let f = fit(&[4], 1800.0, 120.0, 64.0, 92.0).unwrap();
        assert_eq!(f.cols, 4);
        assert!((f.w - f.h * STRETCH).abs() < 0.01);
        // Too small for full tiles.
        assert_eq!(fit(&[8, 8], 300.0, 150.0, 64.0, 92.0), None);
        assert_eq!(fit(&[], 300.0, 150.0, 64.0, 92.0), None);
    }
}
