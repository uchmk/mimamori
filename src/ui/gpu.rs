//! GPU·NPU (4): the devices on the left, the selected one's usage, engines
//! and memory stacked in the middle, its details, what it cannot report and the
//! processes on it on the right.

use egui::{Rect, pos2, vec2};
use ito_layout::{Dir, Node};

use super::{Cx, GAP, Pen, graph, kv_rows, panel, row_at, shown};
use crate::collect::gpu::{self, Device, Fan, Kind};
use crate::config::theme::{self, ROW, ramp};
use crate::i18n::{tr, trf};
use crate::model::Series;
use crate::screen::Pane;
use crate::stacks;
use crate::util::fmt_bytes;
use crate::zoom::{Unit, Zoom};

pub fn draw(cx: &mut Cx, [left, mid, right]: [Rect; 3]) {
    draw_left(cx, left);
    draw_mid(cx, mid);
    draw_right(cx, right);
}

pub(super) fn kind_label(k: Kind) -> &'static str {
    tr(if k == Kind::Npu { "gpu.kind_npu" } else { "gpu.kind_gpu" })
}

/// "GPU0", "NPU1": the device's place among those of its kind.
fn tag(devs: &[Device], i: usize) -> String {
    let k = devs[i].kind;
    let n = devs[..i].iter().filter(|d| d.kind == k).count();
    format!("{}{n}", kind_label(k))
}

/// Its name, or what it is when the source gave none.
pub fn name(d: &Device) -> String {
    if !d.name.is_empty() {
        return d.name.clone();
    }
    match d.vendor {
        Some(v) => trf("gpu.unnamed", &[v, kind_label(d.kind)]),
        None => kind_label(d.kind).to_string(),
    }
}

fn counts(devs: &[Device]) -> (usize, usize) {
    let npus = devs.iter().filter(|d| d.kind == Kind::Npu).count();
    (devs.len() - npus, npus)
}

/// The GPU and NPU counts, for the panel and the crumb.
pub fn summary(devs: &[Device]) -> String {
    let (g, n) = counts(devs);
    trf("gpu.counts", &[&g.to_string(), &n.to_string()])
}

fn pct(v: Option<f32>) -> String {
    v.map_or_else(|| "—".to_string(), |v| format!("{v:.0}%"))
}

/// `s` broken into lines of at most `w` pixels at the spaces, and between
/// any two wide characters (Japanese has no spaces to break at).
fn wrap(pen: &Pen, s: &str, w: f32) -> Vec<String> {
    wrap_at(s, ((w / pen.cw).floor() as usize).max(8))
}

/// [`wrap`] in cells. A word longer than a line (a key, a path) is cut
/// where the lines end.
pub(super) fn wrap_at(s: &str, room: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut used = 0;
    for (unit, spaced) in units(s) {
        let n = crate::util::width(unit);
        if n > room {
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                used = 0;
            }
            for c in unit.chars() {
                let k = crate::util::cells(c);
                if !line.is_empty() && used + k > room {
                    lines.push(std::mem::take(&mut line));
                    used = 0;
                }
                line.push(c);
                used += k;
            }
            continue;
        }
        let mut gap = usize::from(spaced && !line.is_empty());
        if !line.is_empty() && used + gap + n > room {
            lines.push(std::mem::take(&mut line));
            (used, gap) = (0, 0);
        }
        if gap == 1 {
            line.push(' ');
        }
        line.push_str(unit);
        used += gap + n;
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// The pieces a line may break between: a run of narrow characters, or one
/// wide character; each with whether a space came before it.
fn units(s: &str) -> Vec<(&str, bool)> {
    let mut out = Vec::new();
    let (mut start, mut spaced) = (None::<usize>, false);
    for (i, c) in s.char_indices() {
        let wide = crate::util::cells(c) == 2;
        if (c.is_whitespace() || wide)
            && let Some(b) = start.take()
        {
            out.push((&s[b..i], spaced));
            spaced = false;
        }
        if c.is_whitespace() {
            spaced = true;
        } else if wide {
            out.push((&s[i..i + c.len_utf8()], spaced));
            spaced = false;
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(b) = start {
        out.push((&s[b..], spaced));
    }
    out
}

/// Wrapped text from `y` down, as much as fits; returns the `y` under it.
pub(super) fn para(pen: &Pen, body: Rect, mut y: f32, s: &str, color: egui::Color32) -> f32 {
    for l in wrap(pen, s, body.width() - 8.0) {
        if y + ROW > body.bottom() + 0.5 {
            break;
        }
        pen.text(pos2(body.left() + 4.0, y + ROW / 2.0), &pen.fit(&l, body.width() - 8.0), color);
        y += ROW;
    }
    y
}

fn draw_left(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let m = &app.model;
    let info = if m.gpu_read { summary(&m.gpus) } else { String::new() };
    let body = panel(&cx.pen, r, tr("gpu.devices"), &info, cx.focused(Pane::Left));
    if !m.gpu_read {
        cx.pen.text(pos2(body.left() + 4.0, body.top() + ROW / 2.0), tr("misc.reading"), theme::dim());
        return;
    }
    let devs = &m.gpus;
    let list = Rect::from_min_size(body.min, vec2(body.width(), (devs.len() as f32 * ROW).min(body.height())));
    cx.scroll_list(Pane::Left, list, devs.len(), |cx, row, i| {
        let d = &devs[i];
        let cy = row.center().y;
        let color = if d.kind == Kind::Npu { theme::magenta() } else { theme::cyan() };
        let t = cx.pen.text(pos2(row.left() + 6.0, cy), &tag(devs, i), color);
        let rr = cx.pen.text_r(pos2(row.right() - 6.0, cy), &pct(d.usage), d.usage.map_or(theme::dim(), ramp));
        let room = rr.left() - t.right() - cx.pen.cw * 2.0;
        cx.pen.text(pos2(t.right() + cx.pen.cw, cy), &cx.pen.fit(&name(d), room), theme::fg());
    });
    // Why a kind is missing, so an empty list is not a mystery.
    let (g, n) = counts(devs);
    let mut y = list.bottom() + if devs.is_empty() { 0.0 } else { GAP };
    for (none, title, why) in [(g == 0, "gpu.none_gpu", gpu::gpu_note()), (n == 0, "gpu.none_npu", gpu::npu_note())] {
        if !none || y + ROW > body.bottom() {
            continue;
        }
        cx.pen.section(row_at(body, y), tr(title));
        y = para(&cx.pen, body, y + ROW, tr(why.key()), theme::dim()) + GAP;
    }
}

fn draw_mid(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let Some(d) = app.selected_gpu() else {
        let body = panel(&cx.pen, r, tr("gpu.usage"), "", cx.focused(Pane::Mid));
        if app.model.gpu_read {
            para(&cx.pen, body, body.top(), tr("gpu.none"), theme::dim());
        }
        return;
    };
    let (uh, vh) = app.model.gpu_hist.get(&d.id).map_or((None, None), |(u, v)| (Some(u), Some(v)));
    let color = if d.kind == Kind::Npu { theme::magenta() } else { theme::blue() };
    // The usage, the engines when it names them, and the memory, each the
    // same share of the column.
    let engines = !d.engines.is_empty();
    let n = 2 + u8::from(engines);
    let rects = cx.stack(Pane::Mid, r, stacks::chain(Dir::Down, 0, n));

    if shown(rects[0]) {
        let body = panel(&cx.pen, rects[0], &format!("{} · {}", name(d), tr("gpu.usage")), &pct(d.usage), cx.focused(Pane::Mid));
        match uh.filter(|_| d.usage.is_some()) {
            Some(u) => {
                graph::line(&cx.pen, body, u.iter(), 100.0, color);
                let title = format!("{} · {}", name(d), tr("gpu.usage"));
                cx.zoomable(body, 0, Zoom::new(Series::GpuUsage(d.id.clone()), title, color, Unit::Pct));
            }
            None => {
                para(&cx.pen, body, body.top(), "—", theme::dim());
            }
        }
    }

    if engines && shown(rects[1]) {
        let body = panel(&cx.pen, rects[1], tr("gpu.engines"), &d.engines.len().to_string(), false);
        let lw = d.engines.iter().map(|(e, _)| cx.pen.width(e)).fold(0.0, f32::max).min(body.width() * 0.4) + cx.pen.cw * 2.0;
        let mut y = body.top();
        for (e, v) in &d.engines {
            if y + ROW > body.bottom() + 0.5 {
                break;
            }
            let row = row_at(body, y);
            let cy = row.center().y;
            cx.pen.text(pos2(row.left() + 6.0, cy), &cx.pen.fit(e, lw - cx.pen.cw), theme::fg());
            cx.pen.text_r(pos2(row.right() - 6.0, cy), &format!("{v:.0}%"), theme::dim());
            let bar = Rect::from_min_max(pos2(row.left() + 6.0 + lw, cy - 3.0), pos2(row.right() - 6.0 - cx.pen.cw * 5.0, cy + 3.0));
            if bar.width() > 8.0 {
                graph::bar(&cx.pen, bar, *v, color);
            }
            y += ROW;
        }
    }

    let mr = rects[n as usize - 1];
    if !shown(mr) {
        return;
    }
    let body = panel(&cx.pen, mr, tr("gpu.memory"), &pct(d.vram_pct()), false);
    let mut y = body.top();
    let mut bars = vec![(tr("gpu.dedicated"), d.vram_used, d.vram_total, theme::green())];
    if d.shared_total.is_some() || d.shared_used.is_some() {
        bars.push((tr("gpu.shared"), d.shared_used, d.shared_total, theme::magenta()));
    }
    for (label, used, total, c) in bars {
        if y + ROW > body.bottom() + 0.5 {
            return;
        }
        let p = match (used, total) {
            (Some(u), Some(t)) if t > 0 => Some(u as f32 * 100.0 / t as f32),
            _ => None,
        };
        let s = match (used, total) {
            (Some(u), Some(t)) => format!("{} / {}", fmt_bytes(u), fmt_bytes(t)),
            (Some(u), None) => fmt_bytes(u),
            (None, Some(t)) => format!("— / {}", fmt_bytes(t)),
            (None, None) => "—".to_string(),
        };
        cx.pen.pair(row_at(body, y), label, theme::dim(), &s, p.map_or(theme::dim(), ramp));
        y += ROW;
        if let Some(p) = p {
            let bar = Rect::from_min_size(pos2(body.left() + 4.0, y), vec2(body.width() - 8.0, 8.0));
            if bar.bottom() > body.bottom() {
                return;
            }
            graph::bar(&cx.pen, bar, p, c);
            y = bar.bottom() + 6.0;
        }
    }
    if let (Some(v), true) = (vh, d.vram_pct().is_some())
        && y + GAP + ROW + 30.0 <= body.bottom()
    {
        cx.pen.section(row_at(body, y + GAP), &trf("ovw.history", &[tr("gpu.vram")]));
        y += GAP + ROW;
        let g = Rect::from_min_max(pos2(body.left(), y), body.max);
        graph::line(&cx.pen, g, v.iter(), 100.0, theme::green());
        let title = format!("{} · {}", name(d), tr("gpu.vram"));
        cx.zoomable(g, 1, Zoom::new(Series::GpuVram(d.id.clone()), title, theme::green(), Unit::Pct));
    }
}

fn fan(f: Fan) -> String {
    match f {
        Fan::Rpm(r) => format!("{r} rpm"),
        Fan::Percent(p) => format!("{p}%"),
    }
}

fn draw_right(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let d = app.selected_gpu();
    // The details take what they need up to about half, the processes the
    // rest; with nothing selected there are no processes to list either.
    let (top, bottom) = match d {
        Some(d) => {
            let want = 26.0 + 6.0 + ROW * (11.0 + d.why.len() as f32 * 2.0);
            let ratio = want.min(r.height() * 0.55) / r.height().max(1.0);
            let rects = cx.stack(Pane::Right, r, stacks::split(Dir::Down, ratio, Node::Leaf(0), Node::Leaf(1)));
            (rects[0], rects[1])
        }
        None => (r, Rect::NOTHING),
    };

    let Some(d) = d.filter(|_| shown(top)) else {
        if shown(top) {
            panel(&cx.pen, top, tr("gpu.details"), "", false);
        }
        return;
    };
    let body = panel(&cx.pen, top, tr("gpu.details"), "", false);
    let dash = || "—".to_string();
    let rows = [
        (tr("gpu.name"), name(d)),
        (tr("gpu.kind"), kind_label(d.kind).to_string()),
        (tr("gpu.vendor"), d.vendor.map_or_else(dash, str::to_string)),
        (tr("gpu.pci"), d.pci.clone().unwrap_or_else(dash)),
        (tr("gpu.driver"), d.driver.clone().unwrap_or_else(dash)),
        (tr("gpu.source"), d.source.to_string()),
        (tr("gpu.temp"), d.temp.map_or_else(dash, |t| format!("{t:.0} °C"))),
        (tr("gpu.fan"), d.fan.map_or_else(dash, fan)),
        (tr("gpu.power"), d.power.map_or_else(dash, |w| format!("{w:.1} W"))),
        (tr("gpu.clock"), d.clock.map_or_else(dash, |c| format!("{c} MHz"))),
    ];
    let mut y = kv_rows(&cx.pen, body, body.top(), &rows);
    if !d.why.is_empty() && y + GAP + ROW * 2.0 <= body.bottom() {
        y += GAP;
        cx.pen.section(row_at(body, y), tr("gpu.unavailable"));
        y += ROW;
        for &(f, w) in &d.why {
            y = para(&cx.pen, body, y, &format!("{}: {}", tr(f.key()), tr(w.key())), theme::dim());
        }
    }

    if !shown(bottom) {
        return;
    }
    let procs = app.gpu_procs();
    let info = if procs.is_empty() { String::new() } else { procs.len().to_string() };
    let body = panel(&cx.pen, bottom, tr("gpu.procs"), &info, cx.focused(Pane::Right));
    if procs.is_empty() {
        para(
            &cx.pen,
            body,
            body.top(),
            tr(match d.source {
                "sysfs" | "sysfs+fdinfo" => "gpu.procs_none_sysfs",
                "iokit" => "gpu.procs_none_iokit",
                _ => "gpu.procs_none",
            }),
            theme::dim(),
        );
        return;
    }
    cx.scroll_list(Pane::Right, body, procs.len(), |cx, row, i| {
        let g = &procs[i];
        let cy = row.center().y;
        let mem = g.mem.map_or_else(dash, fmt_bytes);
        // Fixed columns from the right: memory, then usage.
        cx.pen.text_r(pos2(row.right() - 6.0, cy), &mem, theme::green());
        let ux = row.right() - 6.0 - cx.pen.cw * 11.0;
        cx.pen.text_r(pos2(ux, cy), &pct(g.usage), g.usage.map_or(theme::dim(), ramp));
        let pr = cx.pen.text(pos2(row.left() + 6.0, cy), &g.pid.to_string(), theme::dim());
        let name = app.model.procs.iter().find(|p| p.pid == g.pid).map_or("?", |p| p.name.as_str());
        let x = (row.left() + 6.0 + cx.pen.cw * 8.0).max(pr.right() + cx.pen.cw);
        cx.pen.text(pos2(x, cy), &cx.pen.fit(name, ux - cx.pen.cw * 6.0 - x), theme::fg());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_wraps_at_spaces_and_between_wide_characters() {
        assert_eq!(wrap_at("no memory of its own", 10), ["no memory", "of its own"]);
        assert_eq!(wrap_at("専用のメモリが無い", 8), ["専用のメ", "モリが無", "い"]);
        assert_eq!(wrap_at("sysfs に使用率が無い", 12), ["sysfs に使用", "率が無い"]);
        // A word longer than a line is cut where the lines end.
        assert_eq!(wrap_at("key abcdefghij xy", 4), ["key", "abcd", "efgh", "ij", "xy"]);
    }
}
