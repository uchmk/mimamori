//! The question before a process is ended: what it is, how it will be
//! ended, and the keys that answer.

use egui::{Color32, Rect, Stroke, StrokeKind, pos2, vec2};

use super::{Pen, kv_rows, panel};
use crate::app::App;
use crate::config::cmd::Act;
use crate::config::keys::render_seq;
use crate::config::theme::{DIM, RED, ROW, YELLOW};
use crate::i18n::{tr, trf};
use crate::kill;

/// The keys of the confirm layer that run `act`, joined the way help joins them.
fn keys_for(app: &App, act: &Act) -> String {
    let keys: Vec<String> =
        app.keymap().confirm.iter().filter(|b| b.run.as_slice() == std::slice::from_ref(act)).map(|b| render_seq(&b.on)).collect();
    keys.join(" / ")
}

fn how() -> String {
    if cfg!(windows) { tr("confirm.how_win").to_string() } else { trf("confirm.how_unix", &[&kill::GRACE.as_secs().to_string()]) }
}

pub fn draw(pen: &Pen, app: &App, full: Rect) {
    let Some(t) = &app.confirm else { return };
    pen.p.rect_filled(full, 0.0, Color32::from_black_alpha(140));
    let w = (full.width() - 40.0).min(560.0);
    let h = (ROW * 7.0 + 32.0).min(full.height() - 40.0);
    if w < 120.0 || h < ROW * 3.0 {
        return;
    }
    let r = Rect::from_center_size(full.center(), vec2(w, h));
    let body = panel(pen, r, tr("confirm.title"), "", true);
    pen.p.rect_stroke(r, 6.0, Stroke::new(1.0, RED.gamma_multiply(0.6)), StrokeKind::Outside);

    let p = app.model.procs.iter().find(|p| p.pid == t.pid);
    let dash = || "—".to_string();
    let rows = [
        (tr("confirm.name"), p.map_or_else(|| t.name.clone(), |p| p.name.clone())),
        (tr("procs.pid"), t.pid.to_string()),
        (tr("procs.user"), p.and_then(|p| p.user.clone()).unwrap_or_else(dash)),
        (tr("procs.cmd"), p.map(|p| p.cmd.clone()).filter(|c| !c.is_empty()).unwrap_or_else(dash)),
    ];
    let clip = pen.clipped(body);
    let y = kv_rows(&clip, body, body.top(), &rows) + ROW / 2.0;
    let how = how();
    clip.text(pos2(body.left() + 4.0, y + ROW / 2.0), &clip.fit(&how, body.width() - 8.0), YELLOW);
    let hint = trf("confirm.hint", &[&keys_for(app, &Act::Confirm), &keys_for(app, &Act::Escape)]);
    clip.text(pos2(body.left() + 4.0, body.bottom() - ROW / 2.0), &clip.fit(&hint, body.width() - 8.0), DIM);
}
