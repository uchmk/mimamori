//! Overview (1): the three gauges and the system on the left, a big graph of
//! the selected gauge over the compact tiles in the middle, the top
//! processes on the right.

use egui::{Color32, Rect, pos2, vec2};

use super::tiles::{tiles, tiles_height};
use super::{Cx, GAP, graph, kv_rows, now_secs, panel, row_at};
use crate::app::{App, OVW_TOP};
use crate::config::theme::{self, ROW, ramp};
use crate::i18n::{tr, trf};
use crate::model::Ring;
use crate::screen::Pane;
use crate::util::{fmt_bytes, fmt_uptime};

/// The gauges in the order of the left column's rows: label, percent,
/// colour and history.
fn gauges(app: &App) -> [(&'static str, f32, Color32, &Ring); 3] {
    let m = &app.model;
    let swap = m.swap_hist.last().unwrap_or(0.0);
    [
        (tr("ovw.cpu"), m.total, theme::blue(), &m.cpu_hist),
        (tr("ovw.mem"), m.mem_pct(), theme::green(), &m.mem_hist),
        (tr("ovw.swap"), swap, theme::magenta(), &m.swap_hist),
    ]
}

pub fn draw(cx: &mut Cx, [left, mid, right]: [Rect; 3]) {
    draw_left(cx, left);
    draw_mid(cx, mid);
    draw_right(cx, right);
}

fn draw_left(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let body = panel(&cx.pen, r, tr("ovw.usage"), "", cx.focused(Pane::Left));
    let mut y = body.top();
    let gs = gauges(app);
    let lw = gs.iter().map(|g| cx.pen.width(g.0)).fold(0.0, f32::max) + cx.pen.cw;
    for (i, (label, pct, color, _)) in gs.into_iter().enumerate() {
        let row = row_at(body, y);
        cx.row_bg(Pane::Left, i, row);
        let cy = row.center().y;
        cx.pen.text(pos2(row.left() + 6.0, cy), label, theme::fg());
        cx.pen.text_r(pos2(row.right() - 6.0, cy), &format!("{pct:.0}%"), ramp(pct));
        let bar = Rect::from_min_max(pos2(row.left() + 6.0 + lw, cy - 3.0), pos2(row.right() - 6.0 - cx.pen.cw * 5.0, cy + 3.0));
        if bar.width() > 8.0 {
            graph::bar(&cx.pen, bar, pct, color);
        }
        y += ROW;
    }
    let Some(st) = &app.model.st else { return };
    y += GAP;
    cx.pen.section(row_at(body, y), tr("ovw.system"));
    y += ROW;
    let up = fmt_uptime(now_secs().saturating_sub(st.boot_time));
    let rows = [
        (tr("sys.host"), st.host.clone()),
        (tr("sys.os"), st.os.clone()),
        (tr("sys.kernel"), st.kernel.clone()),
        (tr("sys.arch"), st.arch.clone()),
        (tr("sys.cpu"), st.brand.clone()),
        (tr("sys.uptime"), up),
    ];
    kv_rows(&cx.pen, body, y, &rows);
}

fn draw_mid(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let sel = cx.sel(Pane::Left).min(2);
    let gs = gauges(app);
    let (label, pct, color, ring) = gs[sel];
    let body = panel(&cx.pen, r, &trf("ovw.history", &[label]), &format!("{pct:.0}%"), cx.focused(Pane::Mid));
    // The tiles take what they need, the two small graphs at the bottom a
    // fixed height, and the graph of the selected gauge the rest.
    let small_h = ROW + 56.0;
    let tiles_h = app.model.st.as_ref().map_or(0.0, |st| ROW + tiles_height(&st.topo, body.width(), false));
    let min_g = 80.0_f32.min(body.height());
    let smalls = body.height() - tiles_h - min_g - GAP * 2.0 > small_h + ROW * 3.0;
    let below = tiles_h + GAP + if smalls { small_h + GAP } else { 0.0 };
    let gh = (body.height() - below).max(min_g);
    let g = Rect::from_min_size(body.min, vec2(body.width(), gh));
    graph::line(&cx.pen, g, ring.iter(), 100.0, color);
    let mut y = g.bottom() + GAP;
    let tiles_bottom = if smalls { body.bottom() - small_h - GAP } else { body.bottom() };

    if let Some(st) = &app.model.st {
        cx.pen.section(row_at(body, y), &format!("{} · {}", tr("ovw.cores"), st.topo.summary()));
        y += ROW;
        let area = Rect::from_min_max(pos2(body.left(), y), pos2(body.right(), tiles_bottom));
        if area.height() > 20.0 {
            tiles(cx, area, false, None, None);
        }
    }

    if smalls {
        let top = body.bottom() - small_h;
        let others: Vec<_> = gs.iter().enumerate().filter(|(i, _)| *i != sel).map(|(_, g)| g).collect();
        let w = (body.width() - GAP) / 2.0;
        for (k, (label, pct, color, ring)) in others.into_iter().enumerate() {
            let x = body.left() + k as f32 * (w + GAP);
            let head = Rect::from_min_size(pos2(x, top), vec2(w, ROW));
            cx.pen.pair(head, label, theme::dim(), &format!("{pct:.0}%"), ramp(*pct));
            let gr = Rect::from_min_max(pos2(x, head.bottom()), pos2(x + w, body.bottom()));
            graph::line(&cx.pen, gr, ring.iter(), 100.0, *color);
        }
    }
}

fn draw_right(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let body = panel(&cx.pen, r, tr("ovw.top"), "", cx.focused(Pane::Right));
    let n_cpu = app.top_cpu.len().min(OVW_TOP);
    let mut y = body.top();
    cx.pen.section(row_at(body, y), tr("ovw.by_cpu"));
    y += ROW;
    for i in 0..app.count(app.screen, Pane::Right) {
        if i == n_cpu {
            y += 4.0;
            if y + ROW > body.bottom() {
                break;
            }
            cx.pen.section(row_at(body, y), tr("ovw.by_mem"));
            y += ROW;
        }
        if y + ROW > body.bottom() + 0.5 {
            break;
        }
        let Some(p) = app.ovw_top(i).and_then(|k| app.proc_at(k)) else { continue };
        let row = row_at(body, y);
        cx.row_bg(Pane::Right, i, row);
        if i < n_cpu {
            cx.pen.pair(row, &p.name, theme::fg(), &format!("{:.1}%", p.cpu), ramp(p.cpu));
        } else {
            cx.pen.pair(row, &p.name, theme::fg(), &fmt_bytes(p.mem), theme::green());
        }
        y += ROW;
    }
}
