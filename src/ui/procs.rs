//! Processes (5): the scope on the left, the sortable list in the middle,
//! the selected process on the right.

use egui::{Rect, pos2, vec2};

use super::{Cx, GAP, graph, kv_rows, panel, row_at};
use crate::app::SortCol;
use crate::collect::Proc;
use crate::config::theme::{BLUE, CYAN, DIM, FG, GREEN, ROW, ramp};
use crate::i18n::{tr, trf};
use crate::screen::Pane;
use crate::util::{fmt_bytes, fmt_epoch};

pub fn draw(cx: &mut Cx, [left, mid, right]: [Rect; 3]) {
    draw_left(cx, left);
    draw_mid(cx, mid);
    draw_right(cx, right);
}

fn draw_left(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let body = panel(&cx.pen, r, tr("procs.scope"), "", cx.focused(Pane::Left));
    let mut y = body.top();
    for (i, (label, mine)) in [(tr("procs.all"), false), (tr("procs.mine"), true)].into_iter().enumerate() {
        if y + ROW > body.bottom() + 0.5 {
            return;
        }
        let row = row_at(body, y);
        cx.row_bg(Pane::Left, i, row);
        cx.pen.pair(row, label, FG, &app.count_scope(mine).to_string(), DIM);
        y += ROW;
    }
    y += GAP;
    let filter = if app.filter.is_empty() { "—".to_string() } else { format!("/{}", app.filter) };
    let rows = [
        (tr("procs.sort"), app.sort.label().to_string()),
        (tr("procs.view"), tr(if app.tree { "procs.tree" } else { "procs.flat" }).to_string()),
        (tr("procs.filter"), filter),
        (tr("procs.user"), app.my_user().unwrap_or("—").to_string()),
    ];
    kv_rows(&cx.pen, body, y, &rows);
}

/// The list's columns: which, how many characters, right-aligned or not.
/// The name takes what the others leave.
const COLS: [(SortCol, usize, bool); 6] = [
    (SortCol::Pid, 7, true),
    (SortCol::Name, 0, false),
    (SortCol::Cpu, 7, true),
    (SortCol::Mem, 9, true),
    (SortCol::Thr, 5, true),
    (SortCol::User, 10, false),
];

/// The columns that fit in `w` pixels, with each one's left edge and width;
/// user goes first, then threads, while the name would be under 10.
fn columns(w: f32, cw: f32) -> Vec<(SortCol, f32, f32, bool)> {
    let mut cols: Vec<(SortCol, usize, bool)> = COLS.to_vec();
    let fixed = |c: &[(SortCol, usize, bool)]| c.iter().map(|&(_, n, _)| n + 1).sum::<usize>() as f32 * cw;
    for drop in [SortCol::User, SortCol::Thr] {
        if w - fixed(&cols) < cw * 10.0 {
            cols.retain(|&(c, _, _)| c != drop);
        }
    }
    let name_w = (w - fixed(&cols)).max(cw * 4.0);
    let mut x = 0.0;
    cols.into_iter()
        .map(|(c, n, right)| {
            let cw_px = if n == 0 { name_w } else { n as f32 * cw };
            let out = (c, x, cw_px, right);
            x += cw_px + cw;
            out
        })
        .collect()
}

fn cell(p: &Proc, c: SortCol) -> String {
    match c {
        SortCol::Pid => p.pid.to_string(),
        SortCol::Name => p.name.clone(),
        SortCol::Cpu => format!("{:.1}", p.cpu),
        SortCol::Mem => fmt_bytes(p.mem),
        SortCol::Thr => p.threads.map_or_else(|| "—".to_string(), |t| t.to_string()),
        SortCol::User => p.user.clone().unwrap_or_else(|| "—".to_string()),
    }
}

/// A row's branch lines in at most `w` pixels: a deep one keeps its end,
/// next to the name, behind an ellipsis.
fn branch(lines: &str, w: f32, cw: f32) -> String {
    let n = lines.chars().count();
    let max = (w / cw) as usize;
    if n <= max {
        return lines.to_string();
    }
    let keep = max.saturating_sub(1);
    let tail: String = lines.chars().skip(n - keep).collect();
    format!("…{tail}")
}

fn draw_mid(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let info = trf("procs.count", &[&app.rows.len().to_string()]);
    let body = panel(&cx.pen, r, tr("procs.title"), &info, cx.focused(Pane::Mid));
    let inner = body.width() - 12.0;
    let cols = columns(inner, cx.pen.cw);
    let x0 = body.left() + 6.0;

    // The header, the sort column lit.
    let head = row_at(body, body.top());
    for &(c, x, w, right) in &cols {
        let color = if c == app.sort { CYAN } else { DIM };
        let label = cx.pen.fit(c.label(), w);
        if right {
            cx.pen.text_r(pos2(x0 + x + w, head.center().y), &label, color);
        } else {
            cx.pen.text(pos2(x0 + x, head.center().y), &label, color);
        }
    }
    let list = Rect::from_min_max(pos2(body.left(), head.bottom()), body.max);
    if app.rows.is_empty() {
        cx.pen.text(pos2(body.left() + 6.0, list.top() + ROW / 2.0), tr("procs.empty"), DIM);
        return;
    }
    cx.scroll_list(Pane::Mid, list, app.rows.len(), |cx, row, i| {
        let Some(p) = app.rows.get(i).and_then(|&k| app.proc_at(k)) else { return };
        let cy = row.center().y;
        for &(c, x, w, right) in &cols {
            let color = match c {
                SortCol::Cpu => ramp(p.cpu.min(100.0)),
                SortCol::Name => FG,
                SortCol::Mem => GREEN,
                _ => DIM,
            };
            if c == SortCol::Name && app.tree {
                let lines = branch(app.tree_lines.get(i).map_or("", String::as_str), w / 2.0, cx.pen.cw);
                let lw = lines.chars().count() as f32 * cx.pen.cw;
                cx.pen.text(pos2(x0 + x, cy), &lines, DIM);
                let s = cx.pen.fit(&p.name, w - lw);
                cx.pen.text(pos2(x0 + x + lw, cy), &s, color);
                continue;
            }
            let s = cx.pen.fit(&cell(p, c), w);
            if right {
                cx.pen.text_r(pos2(x0 + x + w, cy), &s, color);
            } else {
                cx.pen.text(pos2(x0 + x, cy), &s, color);
            }
        }
    });
}

fn draw_right(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let Some(p) = app.selected_proc() else {
        let body = panel(&cx.pen, r, tr("procs.detail"), "", cx.focused(Pane::Right));
        cx.pen.text(pos2(body.left() + 4.0, body.top() + ROW / 2.0), tr("procs.none"), DIM);
        return;
    };
    let body = panel(&cx.pen, r, &p.name, &format!("{:.1}%", p.cpu), cx.focused(Pane::Right));
    let dash = || "—".to_string();
    let rows = [
        (tr("procs.pid"), p.pid.to_string()),
        (tr("procs.parent"), p.ppid.map_or_else(dash, |x| x.to_string())),
        (tr("procs.user"), p.user.clone().unwrap_or_else(dash)),
        (tr("procs.start"), if p.start == 0 { dash() } else { format!("{} UTC", fmt_epoch(p.start)) }),
        (tr("procs.threads"), p.threads.map_or_else(dash, |t| t.to_string())),
        (tr("procs.status"), p.status.clone()),
        (tr("procs.memory"), fmt_bytes(p.mem)),
        (tr("procs.exe"), p.exe.clone().unwrap_or_else(dash)),
        (tr("procs.cmd"), if p.cmd.is_empty() { dash() } else { p.cmd.clone() }),
    ];
    let mut y = kv_rows(&cx.pen, body, body.top(), &rows) + GAP;

    // The last minute of its CPU and memory, sharing what room is left.
    let Some((cpu, mem)) = app.model.proc_hist.get(&p.pid) else { return };
    let room = body.bottom() - y;
    if room < (ROW + 24.0) * 2.0 {
        return;
    }
    let gh = (room - ROW * 2.0 - GAP) / 2.0;
    cx.pen.pair(row_at(body, y), tr("procs.cpu_minute"), DIM, &format!("{:.1}%", p.cpu), ramp(p.cpu.min(100.0)));
    y += ROW;
    let g = Rect::from_min_size(pos2(body.left(), y), vec2(body.width(), gh));
    graph::line(&cx.pen, g, cpu.iter(), cpu.max().max(100.0), BLUE);
    y = g.bottom() + GAP;
    cx.pen.pair(row_at(body, y), tr("procs.mem_minute"), DIM, &fmt_bytes(p.mem), GREEN);
    y += ROW;
    let g = Rect::from_min_size(pos2(body.left(), y), vec2(body.width(), gh));
    graph::line(&cx.pen, g, mem.iter(), mem.max(), GREEN);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_lists_drop_user_then_threads() {
        let names = |w: f32| columns(w, 1.0).into_iter().map(|(c, ..)| c).collect::<Vec<_>>();
        assert_eq!(names(100.0).len(), 6);
        assert!(!names(52.0).contains(&SortCol::User));
        assert!(names(52.0).contains(&SortCol::Thr));
        assert_eq!(names(30.0), vec![SortCol::Pid, SortCol::Name, SortCol::Cpu, SortCol::Mem]);
    }
}
