//! A graph opened full size (double click): the history of its series over
//! the chosen range, with the scale and the time marked, drawn over the
//! screen it came from.

use egui::{Color32, Rect, Stroke, StrokeKind, pos2, vec2};

use super::{Pen, graph, panel};
use crate::app::App;
use crate::config::cmd::Act;
use crate::config::keys::render_seq;
use crate::config::theme::{self, ROW};
use crate::i18n::{tr, trf};
use crate::util::fmt_bytes;
use crate::zoom::Unit;

/// The scale's top over a flat or empty history.
const FLOOR: f32 = 1.0;

fn show(unit: Unit, v: f32) -> String {
    match unit {
        Unit::Pct => format!("{v:.0}%"),
        Unit::Rate => format!("{}/s", fmt_bytes(v.max(0.0) as u64)),
        Unit::Sensor(k) => k.show(v),
    }
}

/// `90 s`, `5 min`, `1 h`: a span of seconds.
fn span(secs: f32) -> String {
    if secs < 120.0 {
        format!("{secs:.0} s")
    } else if secs < 7200.0 {
        format!("{:.0} min", secs / 60.0)
    } else {
        format!("{:.0} h", secs / 3600.0)
    }
}

/// The keys of the zoom layer that run `act`, joined by `/`.
fn keys_for(app: &App, act: Act) -> String {
    let keys: Vec<String> = app.keymap().zoom.iter().filter(|b| b.run == [act]).map(|b| render_seq(&b.on)).collect();
    keys.join(" ")
}

pub fn draw(pen: &Pen, app: &App, full: Rect) {
    let Some(z) = &app.zoom else { return };
    pen.p.rect_filled(full, 0.0, Color32::from_black_alpha(140));
    let w = (full.width() - 60.0).min(1200.0);
    let h = (full.height() - 60.0).min(640.0);
    if w < 200.0 || h < ROW * 8.0 {
        return;
    }
    let r = Rect::from_center_size(full.center(), vec2(w, h));
    let ring = app.model.series(&z.series);
    let data = ring.map(|ring| ring.span(z.range)).unwrap_or_default();
    // What one sample is: the sampler's interval, more for the coarser tiers.
    let step = app.interval() * z.range.per_point() as f32;
    let reach = step * z.range.slots() as f32;
    let now = data.last().map_or_else(String::new, |&v| show(z.unit, v));
    let body = panel(pen, r, &format!("{} · {}", z.title, tr(z.range.key())), &now, true);
    pen.p.rect_stroke(r, 6.0, Stroke::new(1.0, theme::line()), StrokeKind::Outside);

    // The numbers over the range, then the graph, the time under it, and the keys.
    let rows = pen.clipped(body);
    let top = body.top() + ROW / 2.0;
    if data.is_empty() {
        rows.text(pos2(body.left() + 4.0, top), tr("zoom.empty"), theme::dim());
    } else {
        let mean = data.iter().sum::<f32>() / data.len() as f32;
        let least = data.iter().copied().fold(f32::INFINITY, f32::min);
        let most = data.iter().copied().fold(0.0, f32::max);
        let line = format!(
            "{} {}   {} {}   {} {}   {}",
            tr("zoom.avg"),
            show(z.unit, mean),
            tr("zoom.min"),
            show(z.unit, least),
            tr("zoom.max"),
            show(z.unit, most),
            trf("zoom.note", &[&crate::app::fmt_interval((step * 10.0).round() / 10.0)]),
        );
        rows.text(pos2(body.left() + 4.0, top), &rows.fit(&line, body.width() - 8.0), theme::dim());
    }
    let hint = trf("zoom.hint", &[&keys_for(app, Act::Page(1)), &keys_for(app, Act::Page(-1)), &keys_for(app, Act::Escape)]);
    rows.text(pos2(body.left() + 4.0, body.bottom() - ROW / 2.0), &rows.fit(&hint, body.width() - 8.0), theme::dim());

    // The scale's labels set the gutter's width: "10.5 MiB/s" is wider than
    // "7.5 MiB/s", and a label wider than the gutter is cut at the panel's edge.
    let most = data.iter().copied().fold(0.0, f32::max);
    let max = z.max.unwrap_or_else(|| (most * 1.1).max(FLOOR));
    let labels: Vec<String> = (0..=4).map(|q| show(z.unit, max * q as f32 / 4.0)).collect();
    let widest = labels.iter().map(|l| crate::util::width(l)).max().unwrap_or(0);
    let gutter = pen.cw * widest as f32 + 10.0;
    let g = Rect::from_min_max(pos2(body.left() + gutter, body.top() + ROW), pos2(body.right() - 4.0, body.bottom() - ROW * 2.0));
    if g.width() < 40.0 || g.height() < 20.0 {
        return;
    }
    graph::line_over(pen, g, data.iter().copied(), z.range.slots(), max, z.color);
    // The scale at the quarters, the time at the ends and the middle.
    for (q, label) in labels.iter().enumerate() {
        let y = g.bottom() - g.height() * q as f32 / 4.0;
        rows.text_r(pos2(g.left() - 6.0, y.clamp(g.top() + 6.0, g.bottom() - 6.0)), label, theme::dim());
    }
    let y = g.bottom() + ROW / 2.0;
    rows.text(pos2(g.left(), y), &trf("zoom.ago", &[&span(reach)]), theme::dim());
    rows.text_c(pos2(g.center().x, y), &trf("zoom.ago", &[&span(reach / 2.0)]), theme::dim());
    rows.text_r(pos2(g.right(), y), tr("zoom.now"), theme::dim());
}
