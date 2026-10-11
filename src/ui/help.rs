//! The `?` overlay: every binding of each layer, the keys that do the
//! same thing on one line, the way kura's help reads.

use egui::{Color32, Rect, Stroke, StrokeKind, pos2, vec2};

use super::{Pen, panel};
use crate::app::{App, Out};
use crate::config::keymap::Binding;
use crate::config::keys::render_seq;
use crate::config::theme::{self, ROW};
use crate::i18n::tr;

enum Line {
    Head(&'static str),
    Bind(String, String),
    Blank,
}

/// The bindings of a layer, one line per description and command, with the
/// keys in the order the keymap lists them.
fn group(layer: &[Binding]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String, Vec<crate::config::cmd::Act>)> = Vec::new();
    for b in layer {
        let keys = render_seq(&b.on);
        match out.iter_mut().find(|(_, d, run)| *d == b.desc && *run == b.run) {
            Some((k, _, _)) => {
                k.push_str(" / ");
                k.push_str(&keys);
            }
            None => out.push((keys, b.desc.clone(), b.run.clone())),
        }
    }
    out.into_iter().map(|(k, d, _)| (k, crate::i18n::tr_desc(&d).to_string())).collect()
}

fn lines(app: &App) -> Vec<Line> {
    let km = app.keymap();
    let mut out = Vec::new();
    for (title, layer) in [
        (tr("help.main"), &km.main),
        (tr("help.input"), &km.input),
        (tr("help.in_help"), &km.help),
        (tr("help.confirm"), &km.confirm),
        (tr("help.zoom"), &km.zoom),
        (tr("help.settings"), &km.settings),
    ] {
        if !out.is_empty() {
            out.push(Line::Blank);
        }
        out.push(Line::Head(title));
        out.extend(group(layer).into_iter().map(|(k, d)| Line::Bind(k, d)));
    }
    out
}

pub fn draw(pen: &Pen, app: &App, full: Rect, out: &mut Out) {
    pen.p.rect_filled(full, 0.0, Color32::from_black_alpha(140));
    let w = (full.width() - 40.0).min(640.0);
    let h = full.height() - 40.0;
    if w < 120.0 || h < ROW * 3.0 {
        return;
    }
    let r = Rect::from_center_size(full.center(), vec2(w, h));
    let body = panel(pen, r, tr("help.title"), tr("help.close_hint"), true);
    pen.p.rect_stroke(r, 6.0, Stroke::new(1.0, theme::line()), StrokeKind::Outside);

    let all = lines(app);
    let visible = ((body.height() / ROW).floor() as usize).max(1);
    let max = all.len().saturating_sub(visible);
    out.help_max = Some(max);
    let scroll = app.help_scroll.min(max);
    let key_w = (body.width() * 0.4).min(pen.cw * 28.0);
    let clip = pen.clipped(body);
    for (i, line) in all.iter().skip(scroll).take(visible).enumerate() {
        let y = body.top() + i as f32 * ROW + ROW / 2.0;
        match line {
            Line::Head(t) => {
                clip.text(pos2(body.left() + 4.0, y), t, theme::cyan());
            }
            Line::Bind(k, d) => {
                clip.text(pos2(body.left() + 12.0, y), &clip.fit(k, key_w - 16.0), theme::fg());
                clip.text(pos2(body.left() + key_w, y), &clip.fit(d, body.width() - key_w - 4.0), theme::dim());
            }
            Line::Blank => {}
        }
    }
    if max > 0 {
        // Where the view sits in the list.
        let track = Rect::from_min_max(pos2(body.right() - 3.0, body.top()), pos2(body.right(), body.bottom()));
        let th = (track.height() * visible as f32 / all.len() as f32).max(12.0);
        let ty = track.top() + (track.height() - th) * scroll as f32 / max as f32;
        pen.p.rect_filled(Rect::from_min_size(pos2(track.left(), ty), vec2(3.0, th)), 1.5, theme::dim());
    }
}
