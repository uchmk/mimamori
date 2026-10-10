//! The question before a process is ended or a service is started, stopped
//! or restarted: what it is, what will be done, and the keys that answer.

use egui::{Color32, Rect, Stroke, StrokeKind, pos2, vec2};

use super::{Pen, kv_rows, panel};
use crate::app::{App, Ask, Family};
use crate::config::cmd::Act;
use crate::config::keys::render_seq;
use crate::config::theme::{self, ROW};
use crate::i18n::{tr, trf};
use crate::kill;
use crate::svcctl::{self, Action};

/// The keys of the confirm layer that run `act`, joined the way help joins them.
fn keys_for(app: &App, act: &Act) -> String {
    let keys: Vec<String> =
        app.keymap().confirm.iter().filter(|b| b.run.as_slice() == std::slice::from_ref(act)).map(|b| render_seq(&b.on)).collect();
    keys.join(" / ")
}

fn how(family: Family) -> String {
    match family {
        Family::Windows => tr("confirm.how_win").to_string(),
        _ => trf("confirm.how_unix", &[&kill::GRACE.as_secs().to_string()]),
    }
}

pub fn draw(pen: &Pen, app: &App, full: Rect) {
    let Some(ask) = &app.confirm else { return };
    pen.p.rect_filled(full, 0.0, Color32::from_black_alpha(140));
    let dash = || "—".to_string();
    let family = app.family();
    // On another machine: its name, and who its agent acts as.
    let away = app.away_peer().map(|p| (p.name.clone(), p.hello.as_ref().map(|h| h.user.clone()).filter(|u| !u.is_empty())));
    let (title, mut rows, mut how) = match ask {
        Ask::Kill(t) => {
            let p = app.model.procs.iter().find(|p| p.pid == t.pid);
            let rows = vec![
                (tr("confirm.name"), p.map_or_else(|| t.name.clone(), |p| p.name.clone())),
                (tr("procs.pid"), t.pid.to_string()),
                (tr("procs.user"), p.and_then(|p| p.user.clone()).unwrap_or_else(dash)),
                (tr("procs.cmd"), p.map(|p| p.cmd.clone()).filter(|c| !c.is_empty()).unwrap_or_else(dash)),
            ];
            (tr("confirm.title"), rows, vec![how(family)])
        }
        Ask::Svc(t) => {
            let s = app.model.services.list.iter().find(|s| s.name == t.name);
            let rows = vec![
                (tr("confirm.name"), t.name.clone()),
                (tr("svc.display"), s.map(|s| s.display.clone()).filter(|d| !d.is_empty()).unwrap_or_else(dash)),
                (
                    tr("svc.state_row"),
                    s.map_or_else(dash, |s| {
                        if s.sub.is_empty() { tr(s.state.key()).to_string() } else { format!("{} ({})", tr(s.state.key()), s.sub) }
                    }),
                ),
                (tr("svc.start_row"), s.map_or_else(dash, |s| tr(s.start.key()).to_string())),
                (tr("svc.pid"), s.and_then(|s| s.pid).map_or_else(dash, |p| p.to_string())),
            ];
            let title = match t.action {
                Action::Start => tr("confirm.svc_start"),
                Action::Stop => tr("confirm.svc_stop"),
                Action::Restart => tr("confirm.svc_restart"),
            };
            let mut how = Vec::new();
            if away.is_none() {
                how.push(trf("confirm.svc_runs", &[&svcctl::how(t)]));
            }
            if family == Family::Mac && t.action == Action::Stop {
                how.push(tr("confirm.svc_keepalive").to_string());
            }
            (title, rows, how)
        }
    };
    let mut title = title.to_string();
    if let Some((name, user)) = &away {
        title = format!("{name}: {title}");
        let (machine, runs) = match user {
            Some(u) => (trf("confirm.machine_as", &[name, u]), trf("confirm.remote_runs", &[name, u])),
            None => (name.clone(), trf("confirm.remote_runs_plain", &[name])),
        };
        rows.insert(0, (tr("confirm.machine"), machine));
        how.insert(0, runs);
    }
    // The rows, a gap, the lines of how, and the hint below them.
    let w = (full.width() - 40.0).min(560.0);
    let h = (ROW * (rows.len() + how.len()) as f32 + ROW * 2.5).max(ROW * 8.0) + 32.0;
    let h = h.min(full.height() - 40.0);
    if w < 120.0 || h < ROW * 3.0 {
        return;
    }
    let r = Rect::from_center_size(full.center(), vec2(w, h));
    let body = panel(pen, r, &title, "", true);
    pen.p.rect_stroke(r, 6.0, Stroke::new(1.0, theme::red().gamma_multiply(0.6)), StrokeKind::Outside);
    let clip = pen.clipped(body);
    let mut y = kv_rows(&clip, body, body.top(), &rows) + ROW / 2.0;
    for line in &how {
        clip.text(pos2(body.left() + 4.0, y + ROW / 2.0), &clip.fit(line, body.width() - 8.0), theme::yellow());
        y += ROW;
    }
    let hint = trf("confirm.hint", &[&keys_for(app, &Act::Confirm), &keys_for(app, &Act::Escape)]);
    clip.text(pos2(body.left() + 4.0, body.bottom() - ROW / 2.0), &clip.fit(&hint, body.width() - 8.0), theme::dim());
}
