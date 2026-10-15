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

/// The scale's top over a flat or empty history, before it is rounded up.
const FLOOR: f64 = 1.0;

/// The steps a scale's quarters take, times a power of ten.
const NICE: [f64; 9] = [1.0, 1.5, 2.0, 2.5, 3.0, 4.0, 5.0, 6.0, 8.0];

/// The units a rate's scale is shown in.
const RATE: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

fn show(unit: Unit, v: f32) -> String {
    match unit {
        Unit::Pct => format!("{v:.0}%"),
        Unit::Rate => format!("{}/s", fmt_bytes(v.max(0.0).round() as u64)),
        Unit::Sensor(k) => k.show(v),
    }
}

/// The smallest round step at least `raw`; a whole number when `whole`.
fn nice_step(raw: f64, whole: bool) -> f64 {
    if !raw.is_finite() {
        return raw;
    }
    let raw = if whole { raw.max(1.0) } else { raw.max(0.01) };
    let mut p = 10f64.powi(raw.log10().floor() as i32);
    loop {
        for m in NICE {
            let s = m * p;
            if s >= raw * (1.0 - 1e-9) && (!whole || s.fract() == 0.0) {
                return s;
            }
        }
        p *= 10.0;
    }
}

/// How many decimals show `step` and its multiples exactly (at most 2).
fn decimals(step: f64) -> usize {
    (0..2)
        .find(|&d| {
            let x = step * 10f64.powi(d as i32);
            (x - x.round()).abs() < 1e-6
        })
        .unwrap_or(2)
}

/// The scale of a zoomed graph: its top, and the labels at its quarters from
/// the bottom up. Without a fixed top, the top is rounded up so that every
/// quarter is a round number in one unit: an idle link reads 0, 1, 2, 3,
/// 4 B/s, not 0 B/s four times.
fn scale(unit: Unit, max: Option<f32>, most: f32) -> (f32, Vec<String>) {
    if let Some(top) = max {
        return (top, (0..=4).map(|q| show(unit, top * q as f32 / 4.0)).collect());
    }
    // A sample that is not a number (a broken counter) does not set the scale.
    let most = if most.is_finite() { most } else { 0.0 };
    let reach = (f64::from(most) * 1.1).max(FLOOR);
    match unit {
        Unit::Rate => {
            // The unit of the top; its quarters may be fractions of it.
            let mut n = (0..RATE.len()).rev().find(|&n| reach >= 1024f64.powi(n as i32)).unwrap_or(0);
            let mut step = nice_step(reach / 1024f64.powi(n as i32) / 4.0, n == 0);
            // A top rounded past 1024 reads in the next unit: 1.00 KiB/s, not 1200 B/s.
            if step * 4.0 >= 1024.0 && n + 1 < RATE.len() {
                n += 1;
                step = nice_step(reach / 1024f64.powi(n as i32) / 4.0, false);
            }
            let d = decimals(step);
            let top = step * 4.0 * 1024f64.powi(n as i32);
            (top as f32, (0..=4).map(|q| format!("{:.d$} {}/s", step * q as f64, RATE[n])).collect())
        }
        // Shown as whole numbers, so the quarters are whole too.
        Unit::Pct | Unit::Sensor(_) => {
            let step = nice_step(reach / 4.0, true);
            (step as f32 * 4.0, (0..=4).map(|q| show(unit, (step * q as f64) as f32)).collect())
        }
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
    let keys: Vec<String> = crate::config::keymap::live(&app.keymap().zoom).filter(|b| b.run == [act]).map(|b| render_seq(&b.on)).collect();
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
    let (max, labels) = scale(z.unit, z.max, most);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect::sensors::Kind;

    fn labels(unit: Unit, most: f32) -> (f32, Vec<String>) {
        scale(unit, None, most)
    }

    #[test]
    fn an_idle_link_reads_whole_bytes() {
        // #51: an idle link (at most 1 B/s) read 0 B/s at every quarter but the top.
        assert_eq!(labels(Unit::Rate, 1.0), (4.0, vec!["0 B/s".into(), "1 B/s".into(), "2 B/s".into(), "3 B/s".into(), "4 B/s".into()]));
        assert_eq!(labels(Unit::Rate, 0.0).0, 4.0);
    }

    #[test]
    fn a_rate_scale_keeps_one_unit_and_round_quarters() {
        let (top, l) = labels(Unit::Rate, 900.0);
        assert_eq!(top, 1000.0);
        assert_eq!(l, ["0 B/s", "250 B/s", "500 B/s", "750 B/s", "1000 B/s"]);
        let (top, l) = labels(Unit::Rate, 3.0 * 1024.0 * 1024.0);
        assert_eq!(top, 4.0 * 1024.0 * 1024.0);
        assert_eq!(l, ["0 MiB/s", "1 MiB/s", "2 MiB/s", "3 MiB/s", "4 MiB/s"]);
        // Just over a unit: the quarters are fractions of it.
        let (top, l) = labels(Unit::Rate, 1100.0);
        assert!((top - 1.2 * 1024.0).abs() < 0.01, "{top}");
        assert_eq!(l, ["0.0 KiB/s", "0.3 KiB/s", "0.6 KiB/s", "0.9 KiB/s", "1.2 KiB/s"]);
        let (top, l) = labels(Unit::Rate, 2e9);
        assert!((top - 2.4 * 1024f32.powi(3)).abs() < 1e3, "{top}");
        assert_eq!(l, ["0.0 GiB/s", "0.6 GiB/s", "1.2 GiB/s", "1.8 GiB/s", "2.4 GiB/s"]);
        // Rounded up past 1024 B: the next unit, not 1200 B/s.
        let (top, l) = labels(Unit::Rate, 930.0);
        assert_eq!(top, 1024.0);
        assert_eq!(l, ["0.00 KiB/s", "0.25 KiB/s", "0.50 KiB/s", "0.75 KiB/s", "1.00 KiB/s"]);
    }

    #[test]
    fn a_sample_that_is_not_a_number_does_not_set_the_scale() {
        for unit in [Unit::Rate, Unit::Pct, Unit::Sensor(Kind::Temp)] {
            for most in [f32::INFINITY, f32::NAN] {
                assert_eq!(labels(unit, most).0, labels(unit, 0.0).0, "{unit:?} {most}");
            }
        }
    }

    #[test]
    fn a_sensor_scale_has_whole_quarters() {
        // #51: 0..50 °C showed the 12.5 quarter as 12°C.
        let (top, l) = labels(Unit::Sensor(Kind::Temp), 45.0);
        assert_eq!(top, 60.0);
        assert_eq!(l, ["0°C", "15°C", "30°C", "45°C", "60°C"]);
        let (top, l) = labels(Unit::Sensor(Kind::Fan), 1800.0);
        assert_eq!(top, 2000.0);
        assert_eq!(l, ["0 rpm", "500 rpm", "1000 rpm", "1500 rpm", "2000 rpm"]);
        assert_eq!(labels(Unit::Sensor(Kind::Temp), 0.0).0, 4.0);
    }

    #[test]
    fn a_fixed_top_is_kept() {
        assert_eq!(scale(Unit::Pct, Some(100.0), 3.0), (100.0, vec!["0%".into(), "25%".into(), "50%".into(), "75%".into(), "100%".into()]));
    }

    #[test]
    fn every_scale_reaches_the_data_and_its_labels_differ() {
        for unit in [Unit::Rate, Unit::Pct, Unit::Sensor(Kind::Temp)] {
            for most in [0.0, 0.3, 1.0, 3.7, 7.0, 12.5, 99.0, 930.0, 1000.0, 1023.0, 1024.0, 5e5, 3e7, 2e9, 7e12] {
                let (top, l) = labels(unit, most);
                assert!(top >= most * 1.1 - 1e-3 * most, "{unit:?} {most}: {top}");
                // The round steps are at most 1.5 apart; whole steps (1, then 2) may take a small scale up to 8.
                assert!(top <= (most * 1.1 * 1.5).max(8.0) * (1.0 + 1e-6), "{unit:?} {most}: {top} is far over");
                for w in l.windows(2) {
                    assert_ne!(w[0], w[1], "{unit:?} {most}: {l:?}");
                }
            }
        }
    }
}
