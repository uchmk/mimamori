//! The core tiles: one per physical core, grouped by L3 cache (and core
//! kind), with a bar per SMT thread inside. Compact on the overview, full on
//! the CPU screen.

use egui::{Align2, Id, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use super::{Cx, FONT};
use crate::collect::topo::{Core, Topology};
use crate::config::theme::{BG, CYAN, DIM, FG, LINE, ramp};
use crate::i18n::trf;
use crate::screen::Pane;
use crate::util::fmt_bytes;

const TILE_GAP: f32 = 6.0;
const BAR_W: f32 = 10.0;
const BAR_GAP: f32 = 4.0;
/// The line over each group of tiles.
const GROUP_HEAD: f32 = 18.0;

/// A tile's width: room for a bar per thread.
fn tile_w(core: &Core, full: bool) -> f32 {
    let n = core.threads.len().max(1) as f32;
    (n * (BAR_W + BAR_GAP) + 16.0).max(if full { 64.0 } else { 56.0 })
}

fn tile_h(full: bool) -> f32 {
    if full { 92.0 } else { 46.0 }
}

/// How tall the tiles of `topo` are when laid out `width` wide, the way
/// `tiles` wraps them.
pub fn tiles_height(topo: &Topology, width: f32, full: bool) -> f32 {
    let h = tile_h(full);
    let mut y = 0.0;
    for g in &topo.groups {
        y += GROUP_HEAD;
        let mut x = 0.0;
        for core in &g.cores {
            let w = tile_w(core, full);
            if x > 0.0 && x + w > width {
                x = 0.0;
                y += h + TILE_GAP;
            }
            x += w + TILE_GAP;
        }
        y += h + TILE_GAP + 4.0;
    }
    y
}

/// Lay the tiles out in `r`, wrapping; returns the bottom of what was drawn.
/// `sel` is the selected core (its place across the chip) and `click` the
/// column a clicked tile selects in, if tiles are selectable here.
pub fn tiles(cx: &mut Cx, r: Rect, full: bool, sel: Option<usize>, click: Option<Pane>) -> f32 {
    let app = cx.app;
    let Some(st) = &app.model.st else { return r.top() };
    let usage = &app.model.usage;
    let freq = &app.model.freq;
    let topo = &st.topo;
    let h = tile_h(full);
    let mut y = r.top();
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
        cx.pen.text(pos2(r.left() + 2.0, y + 8.0), &cx.pen.fit(&head, r.width()), DIM);
        y += GROUP_HEAD;
        let mut x = r.left();
        for (ci, core) in (start..).zip(&g.cores) {
            let n = core.threads.len().max(1) as f32;
            let w = tile_w(core, full);
            if x > r.left() && x + w > r.right() {
                x = r.left();
                y += h + TILE_GAP;
            }
            if y + h > r.bottom() {
                break;
            }
            let t = Rect::from_min_size(pos2(x, y), vec2(w.min(r.width()), h));
            let vals: Vec<f32> = core.threads.iter().map(|&i| usage.get(i).copied().unwrap_or(0.0)).collect();
            let avg = if vals.is_empty() { 0.0 } else { vals.iter().sum::<f32>() / vals.len() as f32 };
            let selected = sel == Some(ci);
            cx.pen.p.rect_filled(t, 4.0, BG);
            cx.pen.p.rect_stroke(t, 4.0, Stroke::new(1.0, if selected { CYAN } else { LINE }), StrokeKind::Inside);

            // The core's name and its average.
            let top = t.top() + 9.0;
            cx.pen.small(pos2(t.left() + 5.0, top), Align2::LEFT_CENTER, &format!("C{}", core.id), if selected { CYAN } else { DIM });
            cx.pen.small(pos2(t.right() - 5.0, top), Align2::RIGHT_CENTER, &format!("{avg:.0}%"), ramp(avg));

            // A bar per thread, centred.
            let bars_w = n * BAR_W + (n - 1.0) * BAR_GAP;
            let bx = t.center().x - bars_w / 2.0;
            let (b_top, b_bot) = if full { (t.top() + 20.0, t.bottom() - 26.0) } else { (t.top() + 18.0, t.bottom() - 5.0) };
            for (k, (&cpu, &v)) in core.threads.iter().zip(&vals).enumerate() {
                let x0 = bx + k as f32 * (BAR_W + BAR_GAP);
                let track = Rect::from_min_max(pos2(x0, b_top), pos2(x0 + BAR_W, b_bot));
                cx.pen.p.rect_filled(track, 2.0, LINE.gamma_multiply(0.6));
                let fh = track.height() * (v / 100.0).clamp(0.0, 1.0);
                if fh > 0.5 {
                    cx.pen.p.rect_filled(Rect::from_min_max(pos2(track.left(), track.bottom() - fh), track.max), 2.0, ramp(v));
                }
                if full {
                    cx.pen.small(pos2(track.center().x, b_bot + 7.0), Align2::CENTER_CENTER, &cpu.to_string(), DIM);
                }
            }
            if full {
                let mhz: Vec<u64> = core.threads.iter().filter_map(|&i| freq.get(i).copied()).filter(|&f| f > 0).collect();
                if !mhz.is_empty() {
                    let ghz = mhz.iter().sum::<u64>() as f32 / mhz.len() as f32 / 1000.0;
                    cx.pen.small(pos2(t.center().x, t.bottom() - 7.0), Align2::CENTER_CENTER, &format!("{ghz:.1}G"), FG);
                }
            }
            if let Some(p) = click {
                let resp = cx.ui.interact(t, Id::new(("tile", app.screen, ci)), Sense::click());
                if resp.clicked() {
                    cx.out.click = Some((p, ci, resp.double_clicked()));
                }
            }
            x += w + TILE_GAP;
        }
        y += h + TILE_GAP + 4.0;
    }
    y
}
