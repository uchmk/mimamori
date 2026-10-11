//! CPU (2): the groups and counts on the left, the core tiles, the total
//! and the thread heatmap stacked in the middle, the selected core and the top CPU
//! users on the right.

use egui::{Align2, Rect, pos2, vec2};
use ito_layout::Dir;

use super::tiles::tiles;
use super::{Cx, GAP, graph, kv_rows, now_secs, panel, row_at, shown};
use crate::collect::topo::CoreKind;
use crate::config::theme::{self, ROW, ramp};
use crate::i18n::{tr, trf};
use crate::model::Series;
use crate::screen::Pane;
use crate::stacks;
use crate::util::fmt_uptime;
use crate::zoom::{Unit, Zoom};

/// The average usage of some logical CPUs.
fn avg(usage: &[f32], cpus: impl Iterator<Item = usize>) -> f32 {
    let (sum, n) = cpus.fold((0.0, 0), |(s, n), i| (s + usage.get(i).copied().unwrap_or(0.0), n + 1));
    if n == 0 { 0.0 } else { sum / n as f32 }
}

fn kind(k: CoreKind) -> &'static str {
    match k {
        CoreKind::Performance => tr("cpu.kind_p"),
        CoreKind::Efficiency => tr("cpu.kind_e"),
        CoreKind::Unknown => "—",
    }
}

fn ghz(freq: &[u64], cpus: &[usize]) -> Option<String> {
    let mhz: Vec<u64> = cpus.iter().filter_map(|&i| freq.get(i).copied()).filter(|&f| f > 0).collect();
    (!mhz.is_empty()).then(|| format!("{:.2} GHz", mhz.iter().sum::<u64>() as f32 / mhz.len() as f32 / 1000.0))
}

pub fn draw(cx: &mut Cx, [left, mid, right]: [Rect; 3]) {
    draw_left(cx, left);
    draw_mid(cx, mid);
    draw_right(cx, right);
}

fn draw_left(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let m = &app.model;
    let Some(st) = &m.st else { return };
    let body = panel(&cx.pen, r, tr("cpu.groups"), "", cx.focused(Pane::Left));
    let mut y = body.top();
    let all = trf("cpu.all", &[&st.topo.logical.to_string()]);
    let rows = std::iter::once((all, m.total))
        .chain(st.topo.groups.iter().map(|g| (g.label.clone(), avg(&m.usage, g.cores.iter().flat_map(|c| c.threads.iter().copied())))));
    for (i, (label, pct)) in rows.enumerate() {
        if y + ROW > body.bottom() + 0.5 {
            break;
        }
        let row = row_at(body, y);
        cx.row_bg(Pane::Left, i, row);
        cx.pen.pair(row, &label, theme::fg(), &format!("{pct:.0}%"), ramp(pct));
        y += ROW;
    }
    y += GAP;
    if y + ROW > body.bottom() {
        return;
    }
    cx.pen.section(row_at(body, y), tr("cpu.counts"));
    y += ROW;
    let load = m.load.map_or_else(|| "—".to_string(), |(a, b, c)| format!("{a:.2} {b:.2} {c:.2}"));
    let threads = m.total_threads.map_or_else(|| "—".to_string(), |t| t.to_string());
    let rows = [
        (tr("cpu.processes"), m.procs.len().to_string()),
        (tr("cpu.threads"), threads),
        (tr("cpu.load"), load),
        (tr("sys.uptime"), fmt_uptime(now_secs().saturating_sub(st.boot_time))),
        (tr("cpu.topology"), st.topo.source.to_string()),
    ];
    kv_rows(&cx.pen, body, y, &rows);
}

fn draw_mid(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let m = &app.model;
    let Some(st) = &m.st else { return };
    // The cores, the total and the heatmap, each a third of the column.
    let rects = cx.stack(Pane::Mid, r, stacks::chain(Dir::Down, 0, 3));

    if shown(rects[0]) {
        let body = panel(&cx.pen, rects[0], tr("cpu.cores"), &st.topo.summary(), cx.focused(Pane::Mid));
        // The tiles fill the room over the legend.
        let area = Rect::from_min_max(body.min, pos2(body.right(), body.bottom() - ROW));
        let sel = cx.sel(Pane::Mid);
        tiles(cx, area, Some(sel), Some(Pane::Mid));
        // The legend of the usage ramp, at the bottom.
        let cy = body.bottom() - ROW / 2.0;
        let mut x = body.left() + 4.0;
        for (label, v) in [("<30", 15.0), ("30–70", 50.0), ("70–90", 80.0), ("≥90", 95.0)] {
            let sw = Rect::from_center_size(pos2(x + 5.0, cy), vec2(10.0, 10.0));
            cx.pen.p.rect_filled(sw, 2.0, ramp(v));
            let t = cx.pen.small(pos2(sw.right() + 5.0, cy), Align2::LEFT_CENTER, label, theme::dim());
            x = t.right() + 14.0;
        }
        let hint = cx.pen.fit(tr("cpu.hint"), body.right() - x - 4.0);
        cx.pen.text_r(pos2(body.right() - 4.0, cy), &hint, theme::dim());
    }

    // The whole chip over the last minute.
    if shown(rects[1]) {
        let body = panel(&cx.pen, rects[1], tr("cpu.total"), &format!("{:.0}%", m.total), false);
        graph::line(&cx.pen, body, m.cpu_hist.iter(), 100.0, theme::blue());
        cx.zoomable(body, 0, Zoom::new(Series::Cpu, tr("cpu.total"), theme::blue(), Unit::Pct));
    }

    // Every thread over the last minute.
    if shown(rects[2]) {
        let body = panel(&cx.pen, rects[2], tr("cpu.heatmap"), &trf("cpu.threads_n", &[&m.threads.len().to_string()]), false);
        graph::heatmap(&cx.pen, body, &m.threads);
    }
}

fn draw_right(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let m = &app.model;
    let Some(st) = &m.st else { return };
    let sel = cx.sel(Pane::Mid);
    let Some(core) = st.topo.cores().nth(sel) else { return };
    let group = st.topo.groups.iter().find(|g| g.cores.iter().any(|c| c.id == core.id));
    let a = avg(&m.usage, core.threads.iter().copied());
    let body = panel(&cx.pen, r, &trf("cpu.core", &[&core.id.to_string()]), &format!("{a:.0}%"), cx.focused(Pane::Right));
    let threads: Vec<String> = core.threads.iter().map(usize::to_string).collect();
    let rows = [
        (tr("cpu.group"), group.map_or_else(|| "—".to_string(), |g| g.label.clone())),
        (tr("cpu.kind"), kind(core.kind).to_string()),
        (tr("cpu.thread_ids"), threads.join(", ")),
        (tr("cpu.clock"), ghz(&m.freq, &core.threads).unwrap_or_else(|| "—".into())),
    ];
    let mut y = kv_rows(&cx.pen, body, body.top(), &rows);

    // A sparkline per thread of the core.
    for &t in &core.threads {
        if y + ROW > body.bottom() + 0.5 {
            return;
        }
        let row = row_at(body, y);
        let v = m.usage.get(t).copied().unwrap_or(0.0);
        let cy = row.center().y;
        cx.pen.text(pos2(row.left() + 6.0, cy), &format!("T{t}"), theme::dim());
        cx.pen.text_r(pos2(row.right() - 6.0, cy), &format!("{v:.0}%"), ramp(v));
        let sr = Rect::from_min_max(
            pos2(row.left() + 6.0 + cx.pen.cw * 5.0, row.top() + 3.0),
            pos2(row.right() - 6.0 - cx.pen.cw * 5.0, row.bottom() - 3.0),
        );
        if let Some(ring) = m.threads.get(t) {
            graph::spark(&cx.pen, sr, ring.iter(), 100.0, ramp(v));
        }
        y += ROW;
    }

    // The processes using the most CPU.
    y += GAP;
    if y + ROW * 2.0 > body.bottom() {
        return;
    }
    cx.pen.section(row_at(body, y), tr("cpu.top"));
    y += ROW;
    let list = Rect::from_min_max(pos2(body.left(), y), body.max);
    cx.scroll_list(Pane::Right, list, app.top_cpu.len(), |cx, row, i| {
        if let Some(p) = app.top_cpu.get(i).and_then(|&k| app.proc_at(k)) {
            cx.pen.pair(row, &p.name, theme::fg(), &format!("{:.1}%", p.cpu), ramp(p.cpu));
        }
    });
}
