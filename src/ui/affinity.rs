//! The editor for the CPUs a process may run on (`A` in procs): what it has
//! now, the line being typed, a box per CPU to click, and the keys that answer.

use egui::{Color32, CursorIcon, Id, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use super::{Pen, kv_rows, panel};
use crate::app::{App, Out};
use crate::config::cmd::Act;
use crate::config::keys::render_seq;
use crate::config::theme::{self, ROW};
use crate::i18n::{tr, trf};

/// The keys of the input layer that run `act`, joined the way help joins them.
fn keys_for(app: &App, act: &Act) -> String {
    let keys: Vec<String> = crate::config::keymap::live(&app.keymap().input)
        .filter(|b| b.run.as_slice() == std::slice::from_ref(act))
        .map(|b| render_seq(&b.on))
        .collect();
    keys.join(" / ")
}

pub fn draw(ui: &Ui, pen: &Pen, app: &App, full: Rect, out: &mut Out) {
    let Some(e) = &app.affinity else { return };
    let title = trf("aff.title", &[&e.t.name, &e.t.pid.to_string()]);
    // Until its CPUs are read there is nothing to choose from.
    let ids = e.ids.as_deref().unwrap_or_default();
    let chosen = crate::affinity::lenient(&e.text, ids);
    let fault = e.ids.as_ref().and_then(|ids| crate::affinity::parse(&e.text, ids).err());
    // A box as wide as the widest number, as many to a line as fit.
    let w = (full.width() - 40.0).min(640.0);
    let digits = ids.last().copied().unwrap_or(0).to_string().len().max(2);
    let (bw, bh, gap) = (pen.cw * digits as f32 + 12.0, ROW + 4.0, 4.0);
    let per = ((w - 20.0 + gap) / (bw + gap)).floor().max(1.0) as usize;
    let lines = ids.len().div_ceil(per).max(1);
    // Now and new, a gap, the boxes, a gap, the count or the fault, the hint.
    let h = ROW * 2.0 + ROW / 2.0 + lines as f32 * (bh + gap) + ROW / 2.0 + ROW * 2.0 + 32.0;
    let h = h.min(full.height() - 40.0);
    pen.p.rect_filled(full, 0.0, Color32::from_black_alpha(140));
    if w < 160.0 || h < ROW * 6.0 {
        // Too small a window for the panel: the line alone, so what is typed is seen.
        let line = format!("{title}: {}▏", e.text);
        pen.text_c(full.center(), &pen.fit(&line, full.width() - 8.0), theme::fg());
        return;
    }
    let r = Rect::from_center_size(full.center(), vec2(w, h));
    let body = panel(pen, r, &title, "", true);
    let clip = pen.clipped(body);
    let now = if e.ids.is_none() { "…" } else { app.affinity_now().unwrap_or("—") };
    let rows = [(tr("aff.now"), now.to_string()), (tr("aff.new"), format!("{}▏", e.text))];
    let top = kv_rows(&clip, body, body.top(), &rows) + ROW / 2.0;
    if e.ids.is_none() {
        clip.text(pos2(body.left() + 4.0, top + bh / 2.0), tr("aff.reading"), theme::dim());
    }
    // The boxes stop above the two lines at the bottom; typing reaches the rest.
    let floor = body.bottom() - ROW * 2.0;
    let mut shown = 0;
    for (k, &i) in ids.iter().enumerate() {
        let at = pos2(body.left() + 4.0 + (k % per) as f32 * (bw + gap), top + (k / per) as f32 * (bh + gap));
        let b = Rect::from_min_size(at, vec2(bw, bh));
        if b.bottom() > floor {
            break;
        }
        shown += 1;
        let on = chosen.binary_search(&i).is_ok();
        let resp = ui.interact(b, Id::new(("aff-cpu", i)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
        clip.p.rect_filled(b, 3.0, if on { theme::cyan() } else { theme::bg() });
        let edge = if resp.hovered() { theme::cyan() } else { theme::line() };
        clip.p.rect_stroke(b, 3.0, Stroke::new(1.0, edge), StrokeKind::Inside);
        clip.text_c(b.center(), &i.to_string(), if on { theme::bg() } else { theme::dim() });
        if resp.clicked() {
            out.cpu_click = Some(i);
        }
    }
    let (said, color) = match &fault {
        Some(f) => (f.message(), theme::red()),
        None if e.ids.is_none() => (String::new(), theme::dim()),
        None => {
            let mut s = trf("aff.count", &[&chosen.len().to_string(), &ids.len().to_string()]);
            let hidden = ids.len() - shown;
            if hidden > 0 {
                s = format!("{s} · {}", trf("aff.more", &[&hidden.to_string()]));
            }
            (s, theme::dim())
        }
    };
    clip.text(pos2(body.left() + 4.0, body.bottom() - ROW * 1.5), &clip.fit(&said, body.width() - 8.0), color);
    let hint = trf("aff.hint", &[&keys_for(app, &Act::Confirm), &keys_for(app, &Act::Escape)]);
    clip.text(pos2(body.left() + 4.0, body.bottom() - ROW / 2.0), &clip.fit(&hint, body.width() - 8.0), theme::dim());
}
