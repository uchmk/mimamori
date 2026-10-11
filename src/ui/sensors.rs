//! Sensors (9): the chips with their hottest reading on the left, every
//! temperature and fan with its range over the minute and its limits in the
//! middle, the selected sensor's graph and where it was read on the right.
//! Enter on a chip jumps to its first sensor, and on a sensor back to its
//! chip.

use egui::{Color32, Rect, Stroke, pos2, vec2};

use super::{Cx, GAP, gpu::para, graph, kv_rows, panel, row_at};
use crate::collect::sensors::{self, Kind, Sensor, Sensors};
use crate::config::theme::{self, ROW};
use crate::i18n::{tr, trf};
use crate::model::Ring;
use crate::screen::Pane;

pub fn draw(cx: &mut Cx, [left, mid, right]: [Rect; 3]) {
    draw_left(cx, left);
    draw_mid(cx, mid);
    draw_right(cx, right);
}

/// Green, yellow within 10 degrees of the limit, red at it or over the
/// critical mark.
fn heat(v: f32, high: Option<f32>, crit: Option<f32>) -> Color32 {
    if sensors::hot(v, high, crit) {
        theme::red()
    } else if v >= sensors::limit(high, crit) - 10.0 {
        theme::yellow()
    } else {
        theme::green()
    }
}

/// A reading's colour: temperatures by heat; fans dim when stopped and red
/// under their least.
fn color(s: &Sensor) -> Color32 {
    match s.kind {
        Kind::Temp => heat(s.value, s.high, s.crit),
        _ if s.value <= 0.0 => theme::dim(),
        Kind::Fan if s.min.is_some_and(|m| s.value < m) => theme::red(),
        _ => theme::cyan(),
    }
}

/// The top of a sensor's graph: its limits or a round default, raised to
/// what the minute reached.
fn scale(s: &Sensor, ring: &Ring) -> f32 {
    match s.kind {
        Kind::Temp => s.crit.or(s.high).unwrap_or(100.0).max(ring.max()),
        Kind::Fan => ring.max().max(s.high.unwrap_or(1000.0)) * 1.1,
        Kind::Duty => 100.0,
    }
}

/// The sensor that speaks for a chip: its hottest temperature, else its
/// first fan.
fn lead(list: &[Sensor]) -> Option<&Sensor> {
    list.iter().filter(|s| s.kind == Kind::Temp).max_by(|a, b| a.value.total_cmp(&b.value)).or(list.first())
}

fn draw_left(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let m = &app.model;
    let s = &m.sensors;
    let chips = s.chips();
    let info = if m.sensor_read { trf("sensors.chip_count", &[&chips.len().to_string()]) } else { String::new() };
    let body = panel(&cx.pen, r, tr("sensors.chips"), &info, cx.focused(Pane::Left));
    if !m.sensor_read {
        cx.pen.text(pos2(body.left() + 4.0, body.top() + ROW / 2.0), tr("misc.reading"), theme::dim());
        return;
    }
    let list = Rect::from_min_size(body.min, vec2(body.width(), (chips.len() as f32 * ROW).min(body.height())));
    cx.scroll_list(Pane::Left, list, chips.len(), |cx, row, i| {
        let c = &chips[i];
        let Some(x) = lead(&s.list[c.first..c.first + c.len]) else { return };
        let cy = row.center().y;
        let rr = cx.pen.text_r(pos2(row.right() - 6.0, cy), &x.kind.show(x.value), color(x));
        let room = rr.left() - row.left() - 6.0 - cx.pen.cw * 2.0;
        let name = cx.pen.fit(c.name, (room * 0.6).max(cx.pen.cw * 6.0));
        let t = cx.pen.text(pos2(row.left() + 6.0, cy), &name, theme::fg());
        let sr = Rect::from_min_max(pos2(t.right() + cx.pen.cw, row.top() + 4.0), pos2(rr.left() - cx.pen.cw, row.bottom() - 4.0));
        if let Some(ring) = m.sensor_hist.get(&x.id) {
            graph::spark(&cx.pen, sr, ring.iter(), scale(x, ring), color(x));
        }
    });
    // The whole machine, and why the list may be short.
    let mut y = list.bottom() + if chips.is_empty() { 0.0 } else { GAP };
    if y + ROW > body.bottom() {
        return;
    }
    cx.pen.section(row_at(body, y), tr("sensors.all"));
    y += ROW;
    y = kv_rows(&cx.pen, body, y, &summary(s)) + GAP;
    if let Some(n) = s.note_key() {
        para(&cx.pen, body, y, tr(n), if s.list.is_empty() { theme::yellow() } else { theme::dim() });
    }
}

fn summary(s: &Sensors) -> Vec<(&'static str, String)> {
    let hottest = s.hottest().map_or_else(|| "—".to_string(), |x| format!("{} ({})", x.kind.show(x.value), x.label));
    vec![
        (tr("sensors.count"), s.list.len().to_string()),
        (tr("sensors.temps"), s.count(Kind::Temp).to_string()),
        (tr("sensors.fans"), (s.count(Kind::Fan) + s.count(Kind::Duty)).to_string()),
        (tr("sensors.hottest"), hottest),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Col {
    Chip,
    Label,
    Now,
    Low,
    High,
    Limit,
    Crit,
}

/// Width in characters (0 shares what is left) and whether it is right-aligned.
const COLS: [(Col, usize, bool); 7] = [
    (Col::Chip, 0, false),
    (Col::Label, 0, false),
    (Col::Now, 9, true),
    (Col::Low, 9, true),
    (Col::High, 9, true),
    (Col::Limit, 9, true),
    (Col::Crit, 7, true),
];

/// The columns that fit in `w` pixels: the chip, the minute's low, the
/// critical mark and the minute's high go, in that order, while the sensor's
/// name would be under 10 characters.
fn columns(w: f32, cw: f32) -> Vec<(Col, f32, f32, bool)> {
    let mut cols = COLS.to_vec();
    let flex = |c: &[(Col, usize, bool)]| {
        let fixed = c.iter().map(|&(_, n, _)| n + 1).sum::<usize>() as f32 * cw;
        (w - fixed) / c.iter().filter(|&&(_, n, _)| n == 0).count() as f32
    };
    for drop in [Col::Chip, Col::Low, Col::Crit, Col::High] {
        if flex(&cols) < cw * 10.0 {
            cols.retain(|&(c, _, _)| c != drop);
        }
    }
    let flex_w = flex(&cols).max(cw * 6.0);
    let mut x = 0.0;
    cols.into_iter()
        .map(|(c, n, right)| {
            let px = if n == 0 { flex_w } else { n as f32 * cw };
            let out = (c, x, px, right);
            x += px + cw;
            out
        })
        .collect()
}

fn head(c: Col) -> &'static str {
    match c {
        Col::Chip => tr("sensors.col.chip"),
        Col::Label => tr("sensors.col.sensor"),
        Col::Now => tr("sensors.col.now"),
        Col::Low => tr("sensors.col.low"),
        Col::High => tr("sensors.col.high"),
        Col::Limit => tr("sensors.col.limit"),
        Col::Crit => tr("sensors.col.crit"),
    }
}

/// `first`: the first sensor of its chip, which alone names the chip.
fn cell(s: &Sensor, ring: Option<&Ring>, first: bool, col: Col) -> (String, Color32) {
    let dash = || ("—".to_string(), theme::dim());
    let opt = |v: Option<f32>, c: Color32| v.map_or_else(dash, |v| (s.kind.show(v), c));
    match col {
        Col::Chip => (if first { s.chip.clone() } else { String::new() }, theme::dim()),
        Col::Label => (s.label.clone(), theme::fg()),
        Col::Now => (s.kind.show(s.value), color(s)),
        Col::Low => opt(ring.map(Ring::min), theme::dim()),
        Col::High => opt(ring.map(Ring::max), theme::dim()),
        Col::Limit => opt(s.high, theme::dim()),
        Col::Crit => opt(s.crit, theme::dim()),
    }
}

fn draw_mid(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let m = &app.model;
    let list = &m.sensors.list;
    let info = if m.sensor_read { trf("sensors.sensor_count", &[&list.len().to_string()]) } else { String::new() };
    let body = panel(&cx.pen, r, tr("sensors.sensors"), &info, cx.focused(Pane::Mid));
    if !m.sensor_read {
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
    let rows = Rect::from_min_max(pos2(body.left(), h.bottom()), body.max);
    if list.is_empty() {
        para(&cx.pen, rows, rows.top(), tr("sensors.empty"), theme::dim());
        return;
    }
    cx.scroll_list(Pane::Mid, rows, list.len(), |cx, row, i| {
        let s = &list[i];
        let first = i == 0 || list[i - 1].chip_id != s.chip_id;
        let ring = m.sensor_hist.get(&s.id);
        let cy = row.center().y;
        for &(col, x, w, right) in &cols {
            let (text, color) = cell(s, ring, first, col);
            let text = cx.pen.fit(&text, w);
            if right {
                cx.pen.text_r(pos2(x0 + x + w, cy), &text, color);
            } else {
                cx.pen.text(pos2(x0 + x, cy), &text, color);
            }
        }
    });
}

fn detail_rows(s: &Sensor, ring: Option<&Ring>) -> Vec<(&'static str, String)> {
    let dash = || "—".to_string();
    let show = |v: Option<f32>| v.map_or_else(dash, |v| s.kind.show(v));
    let mut rows = vec![
        (tr("sensors.chip"), s.chip.clone()),
        (tr("sensors.sensor"), s.label.clone()),
        (tr("sensors.type"), tr(s.kind.key()).to_string()),
        (tr("sensors.now"), s.kind.show(s.value)),
        (tr("sensors.low"), show(ring.map(Ring::min))),
        (tr("sensors.high"), show(ring.map(Ring::max))),
        (tr("sensors.limit"), show(s.high)),
    ];
    match s.kind {
        Kind::Temp => rows.push((tr("sensors.crit"), show(s.crit))),
        Kind::Fan => rows.push((tr("sensors.min"), show(s.min))),
        Kind::Duty => {}
    }
    rows.push((tr("sensors.source"), s.source.clone()));
    rows
}

/// A faint line across the graph at `v`, for the limits.
fn mark(cx: &Cx, r: Rect, v: Option<f32>, max: f32, color: Color32) {
    let Some(v) = v.filter(|&v| v > 0.0 && v <= max) else { return };
    let inner = r.shrink(1.0);
    let y = inner.bottom() - inner.height() * v / max;
    cx.pen.p.line_segment([pos2(inner.left(), y), pos2(inner.right(), y)], Stroke::new(1.0, color.gamma_multiply(0.5)));
}

fn draw_right(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let m = &app.model;
    let Some(s) = app.selected_sensor() else {
        let body = panel(&cx.pen, r, tr("sensors.detail"), "", cx.focused(Pane::Right));
        if m.sensor_read {
            let y = para(&cx.pen, body, body.top(), tr("sensors.none"), theme::dim()) + GAP;
            if let Some(n) = m.sensors.note_key() {
                para(&cx.pen, body, y, tr(n), theme::yellow());
            }
        }
        return;
    };
    let body = panel(&cx.pen, r, &s.label, &s.chip, cx.focused(Pane::Right));
    let mut y = body.top();
    let ring = m.sensor_hist.get(&s.id);
    if let Some(ring) = ring {
        let h = (body.height() * 0.3).clamp(40.0, 160.0);
        if y + ROW + h <= body.bottom() {
            cx.pen.pair(row_at(body, y), tr(s.kind.key()), theme::dim(), &s.kind.show(s.value), color(s));
            y += ROW;
            let g = Rect::from_min_size(pos2(body.left(), y), vec2(body.width(), h));
            let max = scale(s, ring);
            graph::line(&cx.pen, g, ring.iter(), max, color(s));
            if s.kind == Kind::Temp {
                mark(cx, g, s.high, max, theme::yellow());
                mark(cx, g, s.crit, max, theme::red());
            } else {
                mark(cx, g, s.min, max, theme::red());
            }
            y += h + GAP;
        }
    }
    kv_rows(&cx.pen, body, y, &detail_rows(s, ring));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_lists_drop_the_chip_low_crit_and_high() {
        let names = |w: f32| columns(w, 1.0).into_iter().map(|(c, ..)| c).collect::<Vec<_>>();
        assert_eq!(names(200.0).len(), 7);
        assert_eq!(names(60.0), [Col::Label, Col::Now, Col::Low, Col::High, Col::Limit, Col::Crit]);
        assert_eq!(names(40.0), [Col::Label, Col::Now, Col::Limit]);
        // The chip and the sensor share what is left.
        let cols = columns(200.0, 1.0);
        assert_eq!(cols[0].2, cols[1].2);
    }

    #[test]
    fn temperatures_warm_towards_their_limit() {
        assert_eq!(heat(50.0, None, None), theme::green());
        assert_eq!(heat(82.0, None, None), theme::yellow());
        assert_eq!(heat(90.0, None, None), theme::red());
        // The chip's own marks: high first, then critical.
        assert_eq!(heat(65.0, Some(70.0), Some(100.0)), theme::yellow());
        assert_eq!(heat(55.0, None, Some(60.0)), theme::yellow());
        assert_eq!(heat(101.0, Some(110.0), Some(100.0)), theme::red());
    }

    #[test]
    fn stopped_fans_dim_and_slow_ones_turn_red() {
        let fan = |v: f32, min| Sensor { kind: Kind::Fan, value: v, min, ..Sensor::default() };
        assert_eq!(color(&fan(0.0, Some(300.0))), theme::dim());
        assert_eq!(color(&fan(200.0, Some(300.0))), theme::red());
        assert_eq!(color(&fan(1200.0, Some(300.0))), theme::cyan());
        assert_eq!(color(&Sensor { kind: Kind::Duty, value: 40.0, ..Sensor::default() }), theme::cyan());
    }

    #[test]
    fn a_chip_speaks_through_its_hottest_temperature() {
        let s = |kind, v: f32| Sensor { kind, value: v, ..Sensor::default() };
        let list = [s(Kind::Fan, 900.0), s(Kind::Temp, 40.0), s(Kind::Temp, 60.0)];
        assert_eq!(lead(&list).map(|x| x.value), Some(60.0));
        assert_eq!(lead(&list[..1]).map(|x| x.value), Some(900.0));
        assert!(lead(&[]).is_none());
    }
}
