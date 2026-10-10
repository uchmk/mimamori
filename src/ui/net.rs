//! Network (7): the interfaces with a minute of traffic on the left, the
//! sockets in the middle, the selected interface's graphs and addresses
//! and the selected socket on the right. Enter on a socket opens its
//! process in procs.

use egui::{Color32, Rect, pos2, vec2};

use super::{Cx, GAP, gpu::para, graph, kv_rows, panel, row_at};
use crate::collect::net::{Conn, Nic, State};
use crate::config::theme::{CYAN, DIM, FG, GREEN, MAGENTA, ROW, YELLOW};
use crate::i18n::{tr, trf};
use crate::screen::Pane;
use crate::util::fmt_bytes;

/// Below this the graphs do not scale up, so an idle link stays flat.
const FLOOR: f32 = 1024.0;

pub fn draw(cx: &mut Cx, [left, mid, right]: [Rect; 3]) {
    draw_left(cx, left);
    draw_mid(cx, mid);
    draw_right(cx, right);
}

fn rate(b: f64) -> String {
    format!("{}/s", fmt_bytes(b as u64))
}

fn state_color(s: State) -> Color32 {
    match s {
        State::Listen => CYAN,
        State::Established => GREEN,
        State::SynSent | State::SynRecv => YELLOW,
        _ => DIM,
    }
}

fn draw_left(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let m = &app.model;
    let info = if m.nic_read { trf("net.up_count", &[&m.nics.iter().filter(|n| n.up).count().to_string()]) } else { String::new() };
    let body = panel(&cx.pen, r, tr("net.nics"), &info, cx.focused(Pane::Left));
    if !m.nic_read {
        cx.pen.text(pos2(body.left() + 4.0, body.top() + ROW / 2.0), tr("misc.reading"), DIM);
        return;
    }
    let nics = &m.nics;
    if nics.is_empty() {
        para(&cx.pen, body, body.top(), tr("net.none_nic"), DIM);
    }
    let list = Rect::from_min_size(body.min, vec2(body.width(), (nics.len() as f32 * ROW).min(body.height())));
    cx.scroll_list(Pane::Left, list, nics.len(), |cx, row, i| {
        let n = &nics[i];
        let cy = row.center().y;
        let rr = if n.up {
            cx.pen.text_r(pos2(row.right() - 6.0, cy), &rate(n.rx_rate + n.tx_rate), CYAN)
        } else {
            cx.pen.text_r(pos2(row.right() - 6.0, cy), tr("net.down"), DIM)
        };
        let room = rr.left() - row.left() - 6.0 - cx.pen.cw * 2.0;
        let name_w = (room * 0.6).max(cx.pen.cw * 6.0);
        let name = cx.pen.fit(&n.name, name_w);
        let t = cx.pen.text(pos2(row.left() + 6.0, cy), &name, if n.up { FG } else { DIM });
        let sr = Rect::from_min_max(pos2(t.right() + cx.pen.cw, row.top() + 4.0), pos2(rr.left() - cx.pen.cw, row.bottom() - 4.0));
        if let (true, Some((rx, tx))) = (n.up, m.net_hist.get(&n.name)) {
            let max = rx.max().max(tx.max()).max(FLOOR);
            graph::spark(&cx.pen, sr, tx.iter(), max, MAGENTA);
            graph::spark(&cx.pen, sr, rx.iter(), max, CYAN);
        }
    });
    if !m.conn_read {
        return;
    }
    // What the socket list holds, and why it may be short.
    let mut y = list.bottom() + GAP;
    if y + ROW > body.bottom() {
        return;
    }
    cx.pen.section(row_at(body, y), tr("net.conns"));
    y += ROW;
    let list = &m.conns.list;
    let count = |f: &dyn Fn(&Conn) -> bool| list.iter().filter(|c| f(c)).count().to_string();
    let filter = if app.filter.is_empty() { "—".to_string() } else { format!("/{}", app.filter) };
    let rows = [
        (tr("net.listening"), count(&|c| c.listening())),
        (tr("net.established"), count(&|c| c.state == State::Established)),
        (tr("net.all_conns"), list.len().to_string()),
        (tr("svc.filter"), filter),
    ];
    y = kv_rows(&cx.pen, body, y, &rows) + GAP;
    if let Some(n) = m.conns.note {
        para(&cx.pen, body, y, tr(n.key()), YELLOW);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Col {
    Proto,
    Local,
    Remote,
    State,
    Pid,
    Process,
}

/// Width in characters (0 shares what is left) and whether it is right-aligned.
const COLS: [(Col, usize, bool); 6] = [
    (Col::Proto, 6, false),
    (Col::Local, 0, false),
    (Col::Remote, 0, false),
    (Col::State, 11, false),
    (Col::Pid, 7, true),
    (Col::Process, 14, false),
];

/// The columns that fit in `w` pixels: the process, then the PID, go while
/// the addresses would be under 16 characters each.
fn columns(w: f32, cw: f32) -> Vec<(Col, f32, f32, bool)> {
    let mut cols = COLS.to_vec();
    let flex = |c: &[(Col, usize, bool)]| {
        let fixed = c.iter().map(|&(_, n, _)| n + 1).sum::<usize>() as f32 * cw;
        (w - fixed) / c.iter().filter(|&&(_, n, _)| n == 0).count() as f32
    };
    for drop in [Col::Process, Col::Pid] {
        if flex(&cols) < cw * 16.0 {
            cols.retain(|&(c, _, _)| c != drop);
        }
    }
    let addr_w = flex(&cols).max(cw * 6.0);
    let mut x = 0.0;
    cols.into_iter()
        .map(|(c, n, right)| {
            let px = if n == 0 { addr_w } else { n as f32 * cw };
            let out = (c, x, px, right);
            x += px + cw;
            out
        })
        .collect()
}

fn head(c: Col) -> &'static str {
    match c {
        Col::Proto => tr("net.col.proto"),
        Col::Local => tr("net.col.local"),
        Col::Remote => tr("net.col.remote"),
        Col::State => tr("net.col.state"),
        Col::Pid => tr("net.col.pid"),
        Col::Process => tr("net.col.process"),
    }
}

fn cell(c: &Conn, col: Col, process: Option<&str>) -> (String, Color32) {
    match col {
        Col::Proto => (c.proto.label().to_string(), DIM),
        Col::Local => (c.local.to_string(), FG),
        Col::Remote => c.remote.map_or(("*".to_string(), DIM), |r| (r.to_string(), FG)),
        Col::State => (tr(c.state.key()).to_string(), state_color(c.state)),
        Col::Pid => (c.pid.map_or_else(String::new, |p| p.to_string()), DIM),
        Col::Process => (process.unwrap_or_default().to_string(), if c.pid.is_some() { FG } else { DIM }),
    }
}

fn draw_mid(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let m = &app.model;
    let info = if m.conn_read { trf("net.count", &[&app.net_rows.len().to_string()]) } else { String::new() };
    let title = if app.net_listen { format!("{} · {}", tr("net.conns"), tr("net.listening")) } else { tr("net.conns").to_string() };
    let body = panel(&cx.pen, r, &title, &info, cx.focused(Pane::Mid));
    if !m.conn_read {
        cx.pen.text(pos2(body.left() + 4.0, body.top() + ROW / 2.0), tr("net.reading"), DIM);
        return;
    }
    let cols = columns(body.width() - 12.0, cx.pen.cw);
    let x0 = body.left() + 6.0;
    let h = row_at(body, body.top());
    for &(c, x, w, right) in &cols {
        let label = cx.pen.fit(head(c), w);
        if right {
            cx.pen.text_r(pos2(x0 + x + w, h.center().y), &label, DIM);
        } else {
            cx.pen.text(pos2(x0 + x, h.center().y), &label, DIM);
        }
    }
    let list = Rect::from_min_max(pos2(body.left(), h.bottom()), body.max);
    if app.net_rows.is_empty() {
        let why = m.conns.note.filter(|_| m.conns.list.is_empty()).map_or(tr("net.empty"), |n| tr(n.key()));
        para(&cx.pen, list, list.top(), why, DIM);
        return;
    }
    cx.scroll_list(Pane::Mid, list, app.net_rows.len(), |cx, row, i| {
        let Some(c) = app.net_rows.get(i).and_then(|&k| m.conns.list.get(k)) else { return };
        let process = app.proc_name(c.pid);
        let cy = row.center().y;
        for &(col, x, w, right) in &cols {
            let (text, color) = cell(c, col, process);
            let text = cx.pen.fit(&text, w);
            if right {
                cx.pen.text_r(pos2(x0 + x + w, cy), &text, color);
            } else {
                cx.pen.text(pos2(x0 + x, cy), &text, color);
            }
        }
    });
}

fn nic_rows(n: &Nic) -> Vec<(&'static str, String)> {
    let dash = || "—".to_string();
    let list = |v: &[String]| if v.is_empty() { dash() } else { v.join(", ") };
    let link = match (n.up, n.speed_mbps) {
        (false, _) => tr("net.down").to_string(),
        (true, Some(s)) => trf("net.speed", &[&s.to_string()]),
        (true, None) => tr("net.up").to_string(),
    };
    vec![
        (tr("net.desc"), if n.desc.is_empty() { dash() } else { n.desc.clone() }),
        (tr("net.type"), tr(n.kind.key()).to_string()),
        (tr("net.link"), link),
        (tr("net.ipv4"), list(&n.ipv4)),
        (tr("net.ipv6"), list(&n.ipv6)),
        (tr("net.mac"), n.mac.clone().unwrap_or_else(dash)),
        (tr("net.mtu"), n.mtu.map_or_else(dash, |m| m.to_string())),
        (tr("net.rx"), trf("net.total", &[&fmt_bytes(n.rx_total)])),
        (tr("net.tx"), trf("net.total", &[&fmt_bytes(n.tx_total)])),
    ]
}

fn draw_right(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let m = &app.model;
    let Some(n) = app.selected_nic() else {
        let body = panel(&cx.pen, r, tr("net.detail"), "", cx.focused(Pane::Right));
        if m.nic_read {
            cx.pen.text(pos2(body.left() + 4.0, body.top() + ROW / 2.0), tr("net.none"), DIM);
        }
        return;
    };
    let body = panel(&cx.pen, r, &n.name, tr(n.kind.key()), cx.focused(Pane::Right));
    let mut y = body.top();
    // A graph each way, so a quiet upload is not flattened by a download.
    if let Some((rx, tx)) = m.net_hist.get(&n.name) {
        let h = (body.height() * 0.16).clamp(28.0, 90.0);
        for (label, ring, now, color) in [("net.rx", rx, n.rx_rate, CYAN), ("net.tx", tx, n.tx_rate, MAGENTA)] {
            if y + ROW + h > body.bottom() {
                break;
            }
            cx.pen.pair(row_at(body, y), tr(label), DIM, &rate(now), color);
            y += ROW;
            graph::line(
                &cx.pen,
                Rect::from_min_size(pos2(body.left(), y), vec2(body.width(), h)),
                ring.iter(),
                ring.max().max(FLOOR),
                color,
            );
            y += h + GAP;
        }
    }
    y = kv_rows(&cx.pen, body, y, &nic_rows(n)) + GAP;

    let Some(c) = app.selected_conn() else { return };
    if y + ROW * 2.0 > body.bottom() {
        return;
    }
    cx.pen.section(row_at(body, y), tr("net.conn"));
    y += ROW;
    let dash = || "—".to_string();
    let rows = [
        (tr("net.proto"), c.proto.label().to_string()),
        (tr("net.local"), c.local.to_string()),
        (tr("net.remote"), c.remote.map_or_else(dash, |r| r.to_string())),
        (tr("net.status"), tr(c.state.key()).to_string()),
        (tr("net.pid"), c.pid.map_or_else(dash, |p| p.to_string())),
        (tr("net.process"), app.proc_name(c.pid).map_or_else(dash, str::to_string)),
        (tr("net.source"), m.conns.source.to_string()),
    ];
    y = kv_rows(&cx.pen, body, y, &rows) + GAP;
    if c.pid.is_some() {
        para(&cx.pen, body, y, tr("net.hint"), DIM);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_lists_drop_the_process_then_the_pid() {
        let names = |w: f32| columns(w, 1.0).into_iter().map(|(c, ..)| c).collect::<Vec<_>>();
        assert_eq!(names(120.0).len(), 6);
        assert_eq!(names(64.0), [Col::Proto, Col::Local, Col::Remote, Col::State, Col::Pid]);
        assert_eq!(names(40.0), [Col::Proto, Col::Local, Col::Remote, Col::State]);
        // The two addresses share what is left.
        let cols = columns(120.0, 1.0);
        assert_eq!(cols[1].2, cols[2].2);
    }
}
