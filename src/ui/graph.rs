//! The hand-drawn graphs: a line over the last minute, a bar, a sparkline,
//! and the thread × time heatmap.

use egui::epaint::Mesh;
use egui::{Color32, Rect, Shape, Stroke, StrokeKind, pos2, vec2};

use super::Pen;
use crate::config::theme::{self, ramp};
use crate::model::{HISTORY, Ring};

/// Where sample `k` of `slots` sits across `r`; the newest is at the right
/// edge, so a history still filling grows from the right.
fn slot_x(r: Rect, k: usize, slots: usize) -> f32 {
    r.left() + r.width() * k as f32 / (slots - 1) as f32
}

/// The values as points across `r`, `max` at the top.
fn points(r: Rect, data: impl ExactSizeIterator<Item = f32>, slots: usize, max: f32) -> Vec<egui::Pos2> {
    let first = slots.saturating_sub(data.len());
    let max = max.max(f32::EPSILON);
    data.enumerate().map(|(i, v)| pos2(slot_x(r, first + i, slots), r.bottom() - r.height() * (v / max).clamp(0.0, 1.0))).collect()
}

/// The line and the area under it.
fn trace(pen: &Pen, r: Rect, pts: &[egui::Pos2], color: Color32) {
    if pts.len() < 2 {
        return;
    }
    let mut mesh = Mesh::default();
    let fill = color.gamma_multiply(0.15);
    for w in pts.windows(2) {
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(w[0], fill);
        mesh.colored_vertex(w[1], fill);
        mesh.colored_vertex(pos2(w[1].x, r.bottom()), fill);
        mesh.colored_vertex(pos2(w[0].x, r.bottom()), fill);
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    pen.p.add(Shape::mesh(mesh));
    pen.p.add(Shape::line(pts.to_vec(), Stroke::new(1.5, color)));
}

/// A minute of `data` (0 to `max`) with gridlines at the quarters.
pub fn line(pen: &Pen, r: Rect, data: impl ExactSizeIterator<Item = f32>, max: f32, color: Color32) {
    line_over(pen, r, data, HISTORY, max, color);
}

/// [`line`] over `slots` samples instead of a minute's.
pub fn line_over(pen: &Pen, r: Rect, data: impl ExactSizeIterator<Item = f32>, slots: usize, max: f32, color: Color32) {
    if r.width() < 4.0 || r.height() < 4.0 {
        return;
    }
    pen.p.rect_filled(r, 3.0, theme::bg());
    for q in 1..4 {
        let y = r.bottom() - r.height() * q as f32 / 4.0;
        pen.p.line_segment([pos2(r.left(), y), pos2(r.right(), y)], Stroke::new(1.0, theme::line().gamma_multiply(0.6)));
    }
    let inner = r.shrink(1.0);
    trace(pen, inner, &points(inner, data, slots, max), color);
    pen.p.rect_stroke(r, 3.0, Stroke::new(1.0, theme::line()), StrokeKind::Inside);
}

/// A small line without the grid, for a row.
pub fn spark(pen: &Pen, r: Rect, data: impl ExactSizeIterator<Item = f32>, max: f32, color: Color32) {
    if r.width() < 4.0 || r.height() < 2.0 {
        return;
    }
    trace(pen, r, &points(r, data, HISTORY, max), color);
}

/// A horizontal bar filled to `pct`.
pub fn bar(pen: &Pen, r: Rect, pct: f32, color: Color32) {
    pen.p.rect_filled(r, 2.0, theme::line().gamma_multiply(0.6));
    let w = r.width() * (pct / 100.0).clamp(0.0, 1.0);
    if w > 0.5 {
        pen.p.rect_filled(Rect::from_min_size(r.min, vec2(w, r.height())), 2.0, color);
    }
}

/// One row per logical CPU, one cell per sample, coloured by the usage ramp
/// and brighter as it rises.
pub fn heatmap(pen: &Pen, r: Rect, rows: &[Ring]) {
    if rows.is_empty() || r.width() < 4.0 || r.height() < 4.0 {
        return;
    }
    pen.p.rect_filled(r, 3.0, theme::bg());
    let h = r.height() / rows.len() as f32;
    let w = r.width() / HISTORY as f32;
    let mut mesh = Mesh::default();
    for (y, ring) in rows.iter().enumerate() {
        let first = HISTORY.saturating_sub(ring.len());
        for (i, v) in ring.iter().enumerate() {
            let x = r.left() + (first + i) as f32 * w;
            let cell = Rect::from_min_size(pos2(x, r.top() + y as f32 * h), vec2(w, h)).shrink(if h > 4.0 { 0.5 } else { 0.0 });
            mesh.add_colored_rect(cell, ramp(v).gamma_multiply(0.25 + 0.75 * (v / 100.0).clamp(0.0, 1.0)));
        }
    }
    pen.p.add(Shape::mesh(mesh));
    pen.p.rect_stroke(r, 3.0, Stroke::new(1.0, theme::line()), StrokeKind::Inside);
}
