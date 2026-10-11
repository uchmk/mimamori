//! Containers (C): what Docker or Podman runs. The scopes on the left
//! (all, running, paused, stopped, with their counts), the containers in
//! the middle with their CPU and memory, the selected one on the right with
//! the last minute of both. y copies its id.

use egui::{Color32, Rect, pos2, vec2};

use super::{Cx, GAP, gpu::para, graph, kv_rows, panel, row_at};
use crate::app::CtScope;
use crate::collect::containers::{Container, State};
use crate::config::theme::{self, ROW, ramp};
use crate::i18n::{tr, trf};
use crate::screen::Pane;
use crate::util::{fmt_bytes, fmt_local};

pub fn draw(cx: &mut Cx, [left, mid, right]: [Rect; 3]) {
    draw_left(cx, left);
    draw_mid(cx, mid);
    draw_right(cx, right);
}

fn color(s: State) -> Color32 {
    match s {
        State::Running => theme::green(),
        State::Paused | State::Restarting => theme::yellow(),
        State::Created | State::Exited | State::Other => theme::dim(),
    }
}

/// `12.3%`, or a dash where there is no rate yet.
fn cpu_text(c: &Container) -> String {
    c.cpu.map_or_else(|| "—".to_string(), |v| format!("{v:.1}%"))
}

fn mem_text(c: &Container) -> String {
    c.mem.map_or_else(|| "—".to_string(), fmt_bytes)
}

fn draw_left(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let read = app.model.containers.is_some();
    let info = app.model.containers.as_ref().and_then(|c| c.runtime).map_or("", |r| tr(r.key()));
    let body = panel(&cx.pen, r, tr("ct.scopes"), info, cx.focused(Pane::Left));
    if !read {
        cx.pen.text(pos2(body.left() + 4.0, body.top() + ROW / 2.0), tr("misc.reading"), theme::dim());
        return;
    }
    let list = app.containers();
    cx.scroll_list(Pane::Left, body, CtScope::ALL.len(), |cx, row, i| {
        let scope = CtScope::ALL[i];
        let n = list.iter().filter(|c| scope.has(c.state)).count();
        let lc = if i == 0 { theme::cyan() } else { theme::fg() };
        cx.pen.pair(row, tr(scope.key()), lc, &n.to_string(), theme::dim());
    });
}

fn draw_mid(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let info = if app.model.containers.is_some() { trf("ct.count", &[&app.ct_rows.len().to_string()]) } else { String::new() };
    let body = panel(&cx.pen, r, tr(app.ct_scope().key()), &info, cx.focused(Pane::Mid));
    let Some(cts) = &app.model.containers else {
        cx.pen.text(pos2(body.left() + 4.0, body.top() + ROW / 2.0), tr("misc.reading"), theme::dim());
        return;
    };
    let mut top = body.top();
    if let Some(n) = cts.note {
        top = para(&cx.pen, body, top, tr(n.key()), theme::yellow());
        if cts.list.is_empty() {
            return;
        }
    }
    let cw = cx.pen.cw;
    let w = body.width() - 12.0;
    // The image, then the memory, go while the name would be under 12 characters.
    let (state_w, cpu_w, mem_w) = (cw * 9.0, cw * 7.0, cw * 9.0);
    let show_mem = w - state_w - cpu_w - mem_w - cw * 3.0 >= cw * 12.0;
    let image_w = (w * 0.3).clamp(cw * 8.0, cw * 24.0);
    let fixed = state_w + cpu_w + cw * 2.0 + if show_mem { mem_w + cw } else { 0.0 };
    let show_image = show_mem && w - fixed - image_w - cw >= cw * 12.0;
    let name_w = w - fixed - if show_image { image_w + cw } else { 0.0 };
    let x0 = body.left() + 6.0;
    let cpu_x = x0 + state_w + cw + name_w + cw;
    let mem_x = cpu_x + cpu_w + cw;
    let h = row_at(body, top);
    let cy = h.center().y;
    cx.pen.text(pos2(x0, cy), tr("ct.col.state"), theme::dim());
    cx.pen.text(pos2(x0 + state_w + cw, cy), tr("ct.col.name"), theme::dim());
    cx.pen.text_r(pos2(cpu_x + cpu_w, cy), tr("ct.col.cpu"), theme::dim());
    if show_mem {
        cx.pen.text_r(pos2(mem_x + mem_w, cy), tr("ct.col.mem"), theme::dim());
    }
    if show_image {
        cx.pen.text(pos2(mem_x + mem_w + cw, cy), tr("ct.col.image"), theme::dim());
    }
    let list = Rect::from_min_max(pos2(body.left(), h.bottom()), body.max);
    if app.ct_rows.is_empty() {
        cx.pen.text(pos2(x0, list.top() + ROW / 2.0), tr("ct.empty"), theme::dim());
        return;
    }
    cx.scroll_list(Pane::Mid, list, app.ct_rows.len(), |cx, row, i| {
        let Some(c) = app.ct_rows.get(i).and_then(|&k| cts.list.get(k)) else { return };
        let cy = row.center().y;
        cx.pen.text(pos2(x0, cy), &cx.pen.fit(tr(c.state.key()), state_w), color(c.state));
        let fg = if c.state.live() { theme::fg() } else { theme::dim() };
        cx.pen.text(pos2(x0 + state_w + cw, cy), &cx.pen.fit(&c.name, name_w), fg);
        let cpu_color = c.cpu.map_or(theme::dim(), |v| ramp(v.min(100.0)));
        cx.pen.text_r(pos2(cpu_x + cpu_w, cy), &cpu_text(c), cpu_color);
        if show_mem {
            cx.pen.text_r(pos2(mem_x + mem_w, cy), &mem_text(c), theme::green());
        }
        if show_image {
            cx.pen.text(pos2(mem_x + mem_w + cw, cy), &cx.pen.fit(&c.image, image_w), theme::dim());
        }
    });
}

fn draw_right(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let Some(c) = app.selected_ct() else {
        let body = panel(&cx.pen, r, tr("ct.detail"), "", cx.focused(Pane::Right));
        if app.model.containers.is_some() {
            cx.pen.text(pos2(body.left() + 4.0, body.top() + ROW / 2.0), tr("ct.none"), theme::dim());
        }
        return;
    };
    let body = panel(&cx.pen, r, &c.name, tr(c.state.key()), cx.focused(Pane::Right));
    let dash = || "—".to_string();
    let limit = match (c.mem, c.mem_limit) {
        (Some(m), Some(l)) => format!("{} / {}", fmt_bytes(m), fmt_bytes(l)),
        (Some(m), None) => fmt_bytes(m),
        _ => dash(),
    };
    let rows = [
        (tr("ct.id"), c.id.clone()),
        (tr("ct.image"), c.image.clone()),
        (tr("ct.status"), if c.status.is_empty() { dash() } else { c.status.clone() }),
        (tr("ct.created"), if c.created == 0 { dash() } else { fmt_local(c.created) }),
        (tr("ct.cpu"), cpu_text(c)),
        (tr("ct.memory"), limit),
        (tr("ct.pids"), c.pids.map_or_else(dash, |p| p.to_string())),
    ];
    let mut y = kv_rows(&cx.pen, body, body.top(), &rows) + GAP;
    // The last minute of its CPU and memory, sharing what room is left.
    let Some((cpu, mem)) = app.model.ct_hist.get(&c.id) else { return };
    let room = body.bottom() - y;
    if room < (ROW + 24.0) * 2.0 {
        return;
    }
    let gh = (room - ROW * 2.0 - GAP) / 2.0;
    let cpu_now = c.cpu.unwrap_or(0.0);
    cx.pen.pair(row_at(body, y), tr("procs.cpu_minute"), theme::dim(), &cpu_text(c), ramp(cpu_now.min(100.0)));
    y += ROW;
    let g = Rect::from_min_size(pos2(body.left(), y), vec2(body.width(), gh));
    graph::line(&cx.pen, g, cpu.iter(), cpu.max().max(100.0), theme::blue());
    y = g.bottom() + GAP;
    cx.pen.pair(row_at(body, y), tr("procs.mem_minute"), theme::dim(), &mem_text(c), theme::green());
    y += ROW;
    let g = Rect::from_min_size(pos2(body.left(), y), vec2(body.width(), gh));
    graph::line(&cx.pen, g, mem.iter(), mem.max(), theme::green());
}
