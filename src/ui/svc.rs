//! Services (6): which states to show on the left, the list in the middle,
//! the selected service on the right. Enter on one that runs opens its
//! process in procs; S, x and R start, stop and restart it after asking
//! (`svcctl`). On Windows, u switches the list between services and
//! drivers.

use egui::{Color32, Rect, pos2};

use super::{Cx, GAP, gpu::para, kv_rows, panel, row_at};
use crate::app::App;
use crate::collect::svc::{Service, Start, State};
use crate::config::theme::{self, ROW};
use crate::i18n::{tr, trf};
use crate::screen::Pane;

pub fn draw(cx: &mut Cx, [left, mid, right]: [Rect; 3]) {
    draw_left(cx, left);
    draw_mid(cx, mid);
    draw_right(cx, right);
}

pub fn color(s: State) -> Color32 {
    match s {
        State::Running => theme::green(),
        State::Starting | State::Stopping => theme::yellow(),
        State::Paused => theme::blue(),
        State::Failed => theme::red(),
        State::Stopped | State::Unknown => theme::dim(),
    }
}

fn show_label(s: Option<State>) -> &'static str {
    s.map_or(tr("svc.all"), |s| tr(s.key()))
}

fn draw_left(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let m = &app.model;
    let body = panel(&cx.pen, r, tr("svc.show"), m.services.source, cx.focused(Pane::Left));
    let mut y = body.top();
    for (i, st) in App::SVC_SHOW.into_iter().enumerate() {
        if y + ROW > body.bottom() + 0.5 {
            return;
        }
        let row = row_at(body, y);
        cx.row_bg(Pane::Left, i, row);
        let n = if m.svc_read { app.count_svc(st).to_string() } else { String::new() };
        cx.pen.pair(row, show_label(st), st.map_or(theme::fg(), color), &n, theme::dim());
        y += ROW;
    }
    if !m.svc_read {
        return;
    }
    y += GAP;
    let starts = |s: Start| app.count_start(s).to_string();
    let filter = if app.filter().is_empty() { "—".to_string() } else { format!("/{}", app.filter()) };
    let kind = tr(if app.svc_drivers { "svc.drivers" } else { "svc.services" });
    let rows = [
        (tr("svc.kind"), kind.to_string()),
        (tr("svc.start.auto"), starts(Start::Auto)),
        (tr("svc.start.manual"), starts(Start::Manual)),
        (tr("svc.start.disabled"), starts(Start::Disabled)),
        (tr("svc.filter"), filter),
    ];
    y = kv_rows(&cx.pen, body, y, &rows) + GAP;
    if let Some(n) = m.services.note {
        para(&cx.pen, body, y, tr(n.key()), theme::yellow());
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Col {
    State,
    Name,
    Start,
    Pid,
}

const COLS: [(Col, usize, bool); 4] = [(Col::State, 9, false), (Col::Name, 0, false), (Col::Start, 9, false), (Col::Pid, 7, true)];

/// The columns that fit in `w` pixels: the start, then the PID, go while
/// the name would be under 10 characters.
fn columns(w: f32, cw: f32) -> Vec<(Col, f32, f32, bool)> {
    let mut cols = COLS.to_vec();
    let fixed = |c: &[(Col, usize, bool)]| c.iter().map(|&(_, n, _)| n + 1).sum::<usize>() as f32 * cw;
    for drop in [Col::Start, Col::Pid] {
        if w - fixed(&cols) < cw * 10.0 {
            cols.retain(|&(c, _, _)| c != drop);
        }
    }
    let name_w = (w - fixed(&cols)).max(cw * 4.0);
    let mut x = 0.0;
    cols.into_iter()
        .map(|(c, n, right)| {
            let px = if n == 0 { name_w } else { n as f32 * cw };
            let out = (c, x, px, right);
            x += px + cw;
            out
        })
        .collect()
}

fn head(c: Col) -> &'static str {
    match c {
        Col::State => tr("svc.col.state"),
        Col::Name => tr("svc.col.name"),
        Col::Start => tr("svc.col.start"),
        Col::Pid => tr("svc.col.pid"),
    }
}

fn cell(s: &Service, c: Col) -> (String, Color32) {
    match c {
        Col::State => (tr(s.state.key()).to_string(), color(s.state)),
        Col::Name => (s.name.clone(), theme::fg()),
        Col::Start => (tr(s.start.key()).to_string(), if s.start == Start::Disabled { theme::dim() } else { theme::cyan() }),
        Col::Pid => (s.pid.map_or_else(String::new, |p| p.to_string()), theme::dim()),
    }
}

fn draw_mid(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let info = if app.model.svc_read { trf("svc.count", &[&app.svc_rows.len().to_string()]) } else { String::new() };
    let body = panel(&cx.pen, r, tr("svc.title"), &info, cx.focused(Pane::Mid));
    if !app.model.svc_read {
        cx.pen.text(pos2(body.left() + 4.0, body.top() + ROW / 2.0), tr("misc.reading"), theme::dim());
        return;
    }
    let cols = columns(body.width() - 12.0, cx.pen.cw);
    let x0 = body.left() + 6.0;
    let h = row_at(body, body.top());
    for &(c, x, w, right) in &cols {
        let label = cx.pen.fit(head(c), w);
        if right {
            cx.pen.text_r(pos2(x0 + x + w, h.center().y), &label, theme::dim());
        } else {
            cx.pen.text(pos2(x0 + x, h.center().y), &label, theme::dim());
        }
    }
    let list = Rect::from_min_max(pos2(body.left(), h.bottom()), body.max);
    if app.svc_rows.is_empty() {
        cx.pen.text(pos2(body.left() + 6.0, list.top() + ROW / 2.0), tr("svc.empty"), theme::dim());
        return;
    }
    cx.scroll_list(Pane::Mid, list, app.svc_rows.len(), |cx, row, i| {
        let Some(s) = app.svc_rows.get(i).and_then(|&k| app.model.services.list.get(k)) else { return };
        let cy = row.center().y;
        for &(c, x, w, right) in &cols {
            let (text, color) = cell(s, c);
            let text = cx.pen.fit(&text, w);
            if right {
                cx.pen.text_r(pos2(x0 + x + w, cy), &text, color);
            } else {
                cx.pen.text(pos2(x0 + x, cy), &text, color);
            }
        }
    });
}

fn draw_right(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let Some(s) = app.selected_svc() else {
        let body = panel(&cx.pen, r, tr("svc.detail"), "", cx.focused(Pane::Right));
        if app.model.svc_read {
            cx.pen.text(pos2(body.left() + 4.0, body.top() + ROW / 2.0), tr("svc.none"), theme::dim());
        }
        return;
    };
    let body = panel(&cx.pen, r, &s.name, tr(s.state.key()), cx.focused(Pane::Right));
    let dash = || "—".to_string();
    let or_dash = |v: &Option<String>| v.clone().unwrap_or_else(dash);
    let state = if s.sub.is_empty() { tr(s.state.key()).to_string() } else { format!("{} ({})", tr(s.state.key()), s.sub) };
    let start = if s.start_raw.is_empty() { tr(s.start.key()).to_string() } else { s.start_raw.clone() };
    let rows = [
        (tr("svc.display"), if s.display.is_empty() { dash() } else { s.display.clone() }),
        (tr("svc.state_row"), state),
        (tr("svc.start_row"), start),
        (tr("svc.pid"), s.pid.map_or_else(dash, |p| p.to_string())),
        (tr("svc.exit"), s.exit.map_or_else(dash, |e| e.to_string())),
        (tr("svc.user"), or_dash(&s.user)),
        (tr("svc.cmd"), or_dash(&s.cmd)),
        (tr("svc.file"), or_dash(&s.file)),
        (tr("svc.source"), app.model.services.source.to_string()),
    ];
    let mut y = kv_rows(&cx.pen, body, body.top(), &rows) + GAP;
    if s.pid.is_some() {
        y = para(&cx.pen, body, y, tr("svc.hint"), theme::dim());
    }
    para(&cx.pen, body, y, tr("svc.control"), theme::dim());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_lists_drop_the_start_then_the_pid() {
        let names = |w: f32| columns(w, 1.0).into_iter().map(|(c, ..)| c).collect::<Vec<_>>();
        assert_eq!(names(100.0).len(), 4);
        assert_eq!(names(36.0), [Col::State, Col::Name, Col::Pid]);
        assert_eq!(names(20.0), [Col::State, Col::Name]);
    }
}
