//! The `?` overlay: every binding of each layer, the keys that do the
//! same thing on one line, the way kura's help reads.

use egui::{Color32, Rect, Stroke, StrokeKind, vec2};

use super::{Pen, font, panel};
use crate::app::{App, Out};
use crate::config::theme::{self, ROW};
use crate::i18n::tr;

fn lines(app: &App) -> Vec<ito_help::Line> {
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
        ito_help::push_layer(&mut out, title, &ito_keymap::help_lines(layer, "noop"), |d| crate::i18n::tr_desc(d).to_string());
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

    let style = ito_help::Style {
        font: font(),
        row_h: ROW,
        head: theme::cyan(),
        keys: theme::fg(),
        desc: theme::dim(),
        warn: theme::yellow(),
        dim: theme::dim(),
        bar: theme::dim(),
    };
    let drawn = ito_help::draw(&pen.p, body, &lines(app), app.help_scroll, &style);
    out.help_max = Some(drawn.max);
}
