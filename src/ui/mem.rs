//! Memory (3): used, available and swap on the left, memory and swap
//! stacked in the middle (a bar and the last minute each), the processes holding the most on the right.

use egui::{Rect, pos2, vec2};
use ito_layout::{Dir, Node};

use super::{Cx, GAP, graph, kv_rows, panel, row_at, shown};
use crate::config::theme::{self, ROW, ramp};
use crate::i18n::tr;
use crate::screen::Pane;
use crate::stacks;
use crate::util::fmt_bytes;

fn pct(part: u64, whole: u64) -> f32 {
    if whole == 0 { 0.0 } else { part as f32 * 100.0 / whole as f32 }
}

pub fn draw(cx: &mut Cx, [left, mid, right]: [Rect; 3]) {
    draw_left(cx, left);
    draw_mid(cx, mid);
    draw_right(cx, right);
}

fn draw_left(cx: &mut Cx, r: Rect) {
    let m = &cx.app.model;
    let mem = m.mem;
    let body = panel(&cx.pen, r, tr("mem.title"), "", cx.focused(Pane::Left));
    let mut y = body.top();
    let rows = [
        (tr("mem.used"), pct(mem.used, mem.total), theme::green()),
        (tr("mem.available"), pct(mem.available, mem.total), theme::blue()),
        (tr("mem.swap"), pct(mem.swap_used, mem.swap_total), theme::magenta()),
    ];
    for (i, (label, p, color)) in rows.into_iter().enumerate() {
        if y + ROW > body.bottom() + 0.5 {
            return;
        }
        let row = row_at(body, y);
        cx.row_bg(Pane::Left, i, row);
        let cy = row.center().y;
        cx.pen.text(pos2(row.left() + 6.0, cy), &cx.pen.fit(label, cx.pen.cw * 10.0), theme::fg());
        cx.pen.text_r(pos2(row.right() - 6.0, cy), &format!("{p:.0}%"), theme::dim());
        let bar =
            Rect::from_min_max(pos2(row.left() + 6.0 + cx.pen.cw * 11.0, cy - 3.0), pos2(row.right() - 6.0 - cx.pen.cw * 5.0, cy + 3.0));
        if bar.width() > 8.0 {
            graph::bar(&cx.pen, bar, p, color);
        }
        y += ROW;
    }
    y += GAP;
    if y + ROW > body.bottom() {
        return;
    }
    cx.pen.section(row_at(body, y), tr("mem.details"));
    y += ROW;
    let rows = [
        (tr("mem.total"), fmt_bytes(mem.total)),
        (tr("mem.used"), fmt_bytes(mem.used)),
        (tr("mem.available"), fmt_bytes(mem.available)),
        (tr("mem.swap_total"), fmt_bytes(mem.swap_total)),
        (tr("mem.swap_used"), fmt_bytes(mem.swap_used)),
    ];
    kv_rows(&cx.pen, body, y, &rows);
}

fn draw_mid(cx: &mut Cx, r: Rect) {
    let m = &cx.app.model;
    let mem = m.mem;
    let swap = mem.swap_total > 0;
    // Memory and swap, half the column each; memory alone without swap.
    let default = if swap { stacks::chain(Dir::Down, 0, 2) } else { Node::Leaf(0) };
    let rects = cx.stack(Pane::Mid, r, default);

    if shown(rects[0]) {
        let info = format!("{} / {}", fmt_bytes(mem.used), fmt_bytes(mem.total));
        let body = panel(&cx.pen, rects[0], tr("mem.usage"), &info, cx.focused(Pane::Mid));
        let mut y = body.top();
        let used = pct(mem.used, mem.total);
        let cached = pct(mem.total.saturating_sub(mem.used).saturating_sub(mem.available), mem.total);
        cx.pen.pair(row_at(body, y), tr("mem.used"), theme::dim(), &format!("{used:.1}%"), ramp(used));
        y += ROW;

        // Used, then what is held but not counted as available, then free.
        let bar = Rect::from_min_size(pos2(body.left() + 4.0, y), vec2(body.width() - 8.0, 10.0));
        if bar.bottom() <= body.bottom() {
            graph::bar(&cx.pen, bar, used, theme::green());
            if cached > 0.5 {
                let x0 = bar.left() + bar.width() * (used / 100.0).clamp(0.0, 1.0);
                let w = bar.width() * (cached / 100.0).clamp(0.0, 1.0);
                cx.pen.p.rect_filled(
                    Rect::from_min_size(pos2(x0, bar.top()), vec2(w.min(bar.right() - x0), bar.height())),
                    2.0,
                    theme::green().gamma_multiply(0.35),
                );
            }
        }
        y = bar.bottom() + 6.0;
        if y + ROW <= body.bottom() {
            let legend = format!("■ {} {}   ■ {} {}", tr("mem.used"), fmt_bytes(mem.used), tr("mem.available"), fmt_bytes(mem.available));
            cx.pen.text(pos2(body.left() + 4.0, y + ROW / 2.0), &cx.pen.fit(&legend, body.width() - 8.0), theme::dim());
            y += ROW;
        }
        if y + 20.0 <= body.bottom() {
            let g = Rect::from_min_max(pos2(body.left(), y), body.max);
            graph::line(&cx.pen, g, m.mem_hist.iter(), 100.0, theme::green());
        }
    }

    if swap && shown(rects[1]) {
        let sp = pct(mem.swap_used, mem.swap_total);
        let info = format!("{} / {}", fmt_bytes(mem.swap_used), fmt_bytes(mem.swap_total));
        let body = panel(&cx.pen, rects[1], tr("mem.swap"), &info, false);
        let mut y = body.top();
        cx.pen.pair(row_at(body, y), tr("mem.used"), theme::dim(), &format!("{sp:.1}%"), ramp(sp));
        y += ROW;
        let bar = Rect::from_min_size(pos2(body.left() + 4.0, y), vec2(body.width() - 8.0, 10.0));
        if bar.bottom() > body.bottom() {
            return;
        }
        graph::bar(&cx.pen, bar, sp, theme::magenta());
        y = bar.bottom() + 6.0;
        if y + 20.0 <= body.bottom() {
            let g = Rect::from_min_max(pos2(body.left(), y), body.max);
            graph::line(&cx.pen, g, m.swap_hist.iter(), 100.0, theme::magenta());
        }
    }
}

fn draw_right(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let m = &app.model;
    let info = if m.mem.swap_total == 0 { tr("mem.no_swap").to_string() } else { String::new() };
    let body = panel(&cx.pen, r, tr("mem.top"), &info, cx.focused(Pane::Right));
    cx.scroll_list(Pane::Right, body, app.top_mem.len(), |cx, row, i| {
        let Some(p) = app.top_mem.get(i).and_then(|&k| app.proc_at(k)) else { return };
        let cy = row.center().y;
        let size = fmt_bytes(p.mem);
        let rr = cx.pen.text_r(pos2(row.right() - 6.0, cy), &size, theme::green());
        let name_w = ((rr.left() - row.left()) * 0.45).max(cx.pen.cw * 6.0);
        cx.pen.text(pos2(row.left() + 6.0, cy), &cx.pen.fit(&p.name, name_w - 6.0 - cx.pen.cw), theme::fg());
        let sr = Rect::from_min_max(pos2(row.left() + name_w + 4.0, row.top() + 4.0), pos2(rr.left() - cx.pen.cw, row.bottom() - 4.0));
        if let Some((_, ring)) = m.proc_hist.get(&p.pid) {
            graph::spark(&cx.pen, sr, ring.iter(), ring.max(), theme::green());
        }
    });
}
