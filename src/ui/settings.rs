//! The settings screen (Ctrl+,), laid out as tsumugi's: `ito-prefs`
//! draws the nav and the cards, and the rows common to every uchmk app (the
//! language, the clock, the theme) write common.toml. yagura's own rows
//! write config.toml; what a row asks for is gathered while the page is
//! drawn and done once it is.

use egui::{Align2, FontId, Rect, Sense, Ui, pos2, vec2};
use ito_common::{Clock, CommonChange, common_path, themes_dir};
use ito_prefs::{Words, button, field, row, section, select, sep, status, switch};

use crate::app::{App, INTERVALS, fmt_interval};
use crate::config::edit::Edit;
use crate::config::settings::{ALLOW, BACKENDS, DIR, Host};
use crate::config::theme::{self, ramp};
use crate::i18n::{current_language, tr, trf};
use crate::remote::{self, proto};

const GENERAL: usize = 0;
const THEME: usize = 1;
const FLEET: usize = 2;
const ADVANCED: usize = 3;

/// What the page asked for.
#[derive(Default)]
struct Asked {
    common: Vec<CommonChange>,
    edits: Vec<Edit>,
    interval: Option<f32>,
    copy: Option<String>,
    open_folder: bool,
    /// The fleet page was shown: read this machine's key.
    key: bool,
    toast: Option<String>,
}

/// The rows on each page, for the search.
fn index() -> Vec<(usize, &'static str)> {
    let w = Words::of(current_language());
    let mut v = vec![
        (GENERAL, w.language),
        (GENERAL, w.scale),
        (GENERAL, tr("settings.interval")),
        (GENERAL, w.clock),
        (GENERAL, w.show_time),
        (GENERAL, w.time_format),
        (GENERAL, w.show_date),
        (GENERAL, w.date_format),
        (GENERAL, w.weekday),
        (THEME, w.mode),
        (THEME, w.themes),
        (THEME, w.preview),
    ];
    for k in [
        "settings.agent",
        "settings.allow_kill",
        "settings.allow_service",
        "settings.port",
        "settings.hosts",
        "settings.add_host",
        "settings.setup",
        "settings.copy_agent",
        "settings.copy_ssh",
        "settings.copy_tailscale",
        "settings.copy_host",
        "settings.copy_config",
        "settings.direct",
        "settings.key",
        "settings.copy_viewer",
        "settings.copy_listen",
        "settings.copy_keyed_host",
    ] {
        v.push((FLEET, tr(k)));
    }
    for k in ["settings.backend", "settings.folder", "settings.common", "settings.warnings"] {
        v.push((ADVANCED, tr(k)));
    }
    v
}

pub fn show(ui: &mut Ui, app: &mut App, keys: ito_prefs::Keys) {
    let lang = current_language();
    let w = Words::of(lang);
    let pages = [
        (tr("settings.general"), tr("settings.general_lead")),
        (tr("settings.theme"), tr("settings.theme_lead")),
        (tr("settings.fleet"), tr("settings.fleet_lead")),
        (tr("settings.advanced"), tr("settings.advanced_lead")),
    ];
    let index = index();
    let nav = ito_prefs::Nav { pages: &pages, index: &index, words: w, file: "config.toml" };
    let mut asked = Asked::default();
    let interval = app.interval;
    let warnings = app.prefs.warnings();
    let prefs = &mut app.prefs;
    let (settings, common, themes, base) = (&prefs.settings, &prefs.common, &prefs.themes, prefs.base.as_deref());
    let drafts = &mut prefs.drafts;
    let key = &prefs.key;
    let config = base.map(crate::config::edit::config_path);
    let out = ito_prefs::show(ui, ito_theme::colors(), &mut prefs.state, &nav, keys, |ui, page, l| match page {
        GENERAL => {
            section(ui, l, tr("settings.display"), |ui| {
                let own = settings.app_language.as_deref().filter(|l| *l != "auto");
                let note = match own {
                    Some(code) => trf("settings.language_own", &[code]),
                    None => tr("settings.language_note").into(),
                };
                let now = common.language.as_deref().unwrap_or("auto");
                asked.common.extend(ito_prefs::language_row(ui, l, w, now, &note));
                sep(ui, l);
                asked.common.extend(ito_prefs::scale_row(ui, l, w, common.scale_or_one(), tr("settings.scale_note")));
                sep(ui, l);
                let secs = |s: f32| trf("settings.seconds", &[&fmt_interval(s)]);
                let mut options: Vec<(f32, String)> = INTERVALS.iter().map(|&s| (s, secs(s))).collect();
                if !INTERVALS.contains(&interval) {
                    options.push((interval, secs(interval)));
                }
                let options: Vec<(u32, &str)> = options.iter().map(|(s, t)| (s.to_bits(), t.as_str())).collect();
                let picked = row(ui, l, tr("settings.interval"), tr("settings.interval_note"), |ui| {
                    select(ui, "interval", interval.to_bits(), &options)
                });
                if let Some(bits) = picked {
                    asked.interval = Some(f32::from_bits(bits));
                }
            });
            asked.common.extend(ito_prefs::clock_card(ui, l, w, &common.clock_or(None)));
        }
        THEME => {
            let (t, d, li) = common.theme_choice(None);
            let note = match base {
                Some(b) => trf("settings.theme_note", &[&themes_dir(b).display().to_string()]),
                None => tr("settings.no_folder").into(),
            };
            asked.common.extend(ito_prefs::theme_page(ui, l, w, themes, (&t, &d, &li), &note, preview));
        }
        FLEET => {
            section(ui, l, tr("settings.agent"), |ui| {
                for (k, (key, note)) in ALLOW
                    .iter()
                    .zip([("settings.allow_kill", "settings.allow_kill_note"), ("settings.allow_service", "settings.allow_service_note")])
                    .enumerate()
                {
                    if k > 0 {
                        sep(ui, l);
                    }
                    let on = settings.allow.iter().any(|a| a == key);
                    if row(ui, l, tr(note.0), tr(note.1), |ui| switch(ui, l, on)) {
                        let allow = ALLOW
                            .iter()
                            .filter(|a| if **a == *key { !on } else { settings.allow.iter().any(|b| b == *a) })
                            .map(|a| a.to_string())
                            .collect();
                        asked.edits.push(Edit::Allow(allow));
                    }
                }
                sep(ui, l);
                let port = settings.port.to_string();
                let typed = row(ui, l, tr("settings.port"), tr("settings.port_note"), |ui| {
                    field(ui, drafts, "port", &port, &proto::PORT.to_string(), 90.0)
                });
                if let Some(t) = typed {
                    match t.trim().parse::<u16>() {
                        Ok(p) if p > 0 => asked.edits.push(Edit::Port(p)),
                        _ => asked.toast = Some(tr("settings.port_bad").into()),
                    }
                }
            });
            section(ui, l, tr("settings.hosts"), |ui| {
                let hosts = &settings.hosts;
                for (i, h) in hosts.iter().enumerate() {
                    let label = trf("settings.host_n", &[&(i + 1).to_string()]);
                    let (mut gone, mut name, mut addr) = (false, None, None);
                    row(ui, l, &label, "", |ui| {
                        gone = button(ui, l, tr("settings.delete"));
                        addr = field(ui, drafts, &format!("host-addr-{i}"), &h.addr, "host:port", 220.0);
                        name = field(ui, drafts, &format!("host-name-{i}"), &h.name, "name", 120.0);
                    });
                    let mut list = hosts.clone();
                    if gone {
                        list.remove(i);
                    } else if let Some(n) = name {
                        list[i].name = if n.trim().is_empty() { list[i].addr.clone() } else { n.trim().to_string() };
                    } else if let Some(a) = addr {
                        if a.trim().is_empty() {
                            asked.toast = Some(tr("settings.addr_empty").into());
                            continue;
                        }
                        list[i].addr = a.trim().to_string();
                    } else {
                        sep(ui, l);
                        continue;
                    }
                    asked.edits.push(Edit::Hosts(list));
                    sep(ui, l);
                }
                if row(ui, l, tr("settings.add_host"), tr("settings.add_host_note"), |ui| button(ui, l, tr("settings.add"))) {
                    let mut list = hosts.clone();
                    let n = list.len() + 1;
                    list.push(Host { name: format!("host{n}"), addr: format!("localhost:{}", proto::PORT), ..Host::default() });
                    asked.edits.push(Edit::Hosts(list));
                }
            });
            section(ui, l, tr("settings.setup"), |ui| {
                let mut lines = vec![
                    ("settings.copy_agent", remote::AGENT_LINE.to_string()),
                    ("settings.copy_ssh", remote::ssh_line()),
                    ("settings.copy_tailscale", remote::tailscale_line()),
                    ("settings.copy_host", remote::host_sample()),
                ];
                if let Some(c) = &config {
                    lines.push(("settings.copy_config", c.display().to_string()));
                }
                for (k, (key, text)) in lines.into_iter().enumerate() {
                    if k > 0 {
                        sep(ui, l);
                    }
                    // The note is the line itself, on one line.
                    let shown = text.trim_end().replace('\n', "  ");
                    if row(ui, l, tr(key), &shown, |ui| button(ui, l, tr("settings.copy"))) {
                        asked.copy = Some(text);
                    }
                }
            });
            asked.key = true;
            section(ui, l, tr("settings.direct"), |ui| match key {
                Some(Ok((public, host))) => {
                    let lines = [
                        ("settings.key", public.clone()),
                        ("settings.copy_viewer", remote::noise::viewer_snippet(host, public)),
                        ("settings.copy_listen", remote::noise::listen_snippet()),
                        ("settings.copy_keyed_host", remote::noise::host_snippet(host, public)),
                    ];
                    for (k, (key, text)) in lines.into_iter().enumerate() {
                        if k > 0 {
                            sep(ui, l);
                        }
                        let shown = text.trim_end().replace('\n', "  ");
                        if row(ui, l, tr(key), &shown, |ui| button(ui, l, tr("settings.copy"))) {
                            asked.copy = Some(text);
                        }
                    }
                }
                Some(Err(e)) => {
                    row(ui, l, tr("settings.key"), "", |_| ());
                    status(ui, e, l.c.err);
                }
                None => {
                    row(ui, l, tr("settings.key"), tr("settings.key_reading"), |_| ());
                }
            });
        }
        ADVANCED => {
            section(ui, l, tr("settings.graphics"), |ui| {
                let now = settings.backend.as_deref().unwrap_or("auto");
                let options: Vec<(&str, &str)> = BACKENDS.iter().map(|b| (*b, *b)).collect();
                if let Some(b) = row(ui, l, tr("settings.backend"), tr("settings.backend_note"), |ui| select(ui, "backend", now, &options))
                {
                    asked.edits.push(Edit::Backend(b.to_string()));
                }
            });
            section(ui, l, tr("settings.files"), |ui| match base {
                Some(b) => {
                    let folder = b.join(DIR).display().to_string();
                    row(ui, l, tr("settings.folder"), &folder, |ui| {
                        if button(ui, l, tr("settings.open")) {
                            asked.open_folder = true;
                        }
                        if button(ui, l, tr("settings.copy")) {
                            asked.copy = Some(folder.clone());
                        }
                    });
                    sep(ui, l);
                    let shared = common_path(b).display().to_string();
                    if row(ui, l, tr("settings.common"), &shared, |ui| button(ui, l, tr("settings.copy"))) {
                        asked.copy = Some(shared.clone());
                    }
                }
                None => {
                    row(ui, l, tr("settings.folder"), tr("settings.no_folder"), |_| ());
                }
            });
            section(ui, l, tr("settings.warnings"), |ui| {
                if warnings.is_empty() {
                    row(ui, l, tr("settings.no_warnings"), "", |_| ());
                }
                for (k, warn) in warnings.iter().enumerate() {
                    if k > 0 {
                        sep(ui, l);
                    }
                    ui.add_space(8.0);
                    status(ui, warn, l.c.err);
                    ui.add_space(8.0);
                }
            });
        }
        _ => {}
    });

    if out.close {
        app.close_settings();
    }
    if out.open_file {
        app.prefs.open(true);
    }
    if asked.open_folder {
        app.prefs.open(false);
    }
    if asked.key {
        app.prefs.ask_key();
    }
    app.prefs.common_change(asked.common);
    for e in asked.edits {
        app.prefs.config_edit(e);
    }
    if let Some(s) = asked.interval {
        // At once, and kept in config.toml for the next start.
        app.set_interval(s);
        app.prefs.config_edit(Edit::Interval(s));
    }
    if let Some(t) = asked.toast {
        app.toast(t);
    }
    if let Some(text) = asked.copy {
        app.copy(text);
    }
    draw_toast(ui, app);
}

/// The toast over the foot of the page: there is no status bar here.
fn draw_toast(ui: &Ui, app: &App) {
    let Some((t, left)) = app.shown_toast() else { return };
    let c = ito_theme::colors();
    let font = FontId::proportional(12.5);
    let area = ui.max_rect();
    let galley = ui.painter().layout_no_wrap(t.to_string(), font, c.strong());
    let size = galley.size() + vec2(28.0, 14.0);
    let r = Rect::from_center_size(pos2(area.center().x + 120.0, area.bottom() - 20.0 - size.y / 2.0), size);
    let p = ui.painter();
    p.rect_filled(r, 8.0, c.panel);
    p.rect_stroke(r, 8.0, egui::Stroke::new(1.0, c.wait), egui::StrokeKind::Inside);
    p.galley(r.center() - galley.size() / 2.0, galley, c.strong());
    ui.ctx().request_repaint_after(left);
}

/// yagura in the theme in force: a row of core tiles, a few processes
/// with one picked, and the status bar with its clock.
fn preview(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width().min(420.0), 236.0), Sense::hover());
    let p = ui.painter_at(r);
    let mono = FontId::monospace(12.0);
    p.rect_filled(r, 8.0, theme::bg());
    p.rect_stroke(r, 8.0, egui::Stroke::new(1.0, theme::line()), egui::StrokeKind::Inside);
    let inner = r.shrink(12.0);
    p.text(inner.left_top(), Align2::LEFT_TOP, tr("tab.cpu"), mono.clone(), theme::cyan());
    // The tiles: one load each, from cool to hot.
    let loads = [6.0, 18.0, 34.0, 52.0, 71.0, 88.0, 97.0, 45.0];
    let tw = (inner.width() - 7.0 * 6.0) / 8.0;
    for (k, pct) in loads.iter().enumerate() {
        let t = Rect::from_min_size(pos2(inner.left() + k as f32 * (tw + 6.0), inner.top() + 22.0), vec2(tw, 40.0));
        p.rect_filled(t, 4.0, theme::bg_alt());
        let fill = Rect::from_min_max(pos2(t.left(), t.bottom() - t.height() * pct / 100.0), t.max);
        p.rect_filled(fill, 4.0, ramp(*pct).gamma_multiply(0.55));
        p.text(t.center(), Align2::CENTER_CENTER, format!("{pct:.0}"), mono.clone(), theme::fg());
    }
    // The processes, the second one picked.
    let rows = [
        ("yagura", "0.8", theme::fg()),
        ("tsumugi", "3.1", theme::fg()),
        ("cargo", "54.0", theme::yellow()),
        ("rustc", "98.2", theme::red()),
    ];
    for (k, (name, cpu, color)) in rows.iter().enumerate() {
        let y = inner.top() + 76.0 + k as f32 * 23.0;
        let line = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), 23.0));
        let fg = if k == 1 {
            p.rect_filled(line, 3.0, theme::sel());
            theme::strong()
        } else {
            *color
        };
        p.text(pos2(line.left() + 6.0, line.center().y), Align2::LEFT_CENTER, *name, mono.clone(), fg);
        p.text(
            pos2(line.right() - 6.0, line.center().y),
            Align2::RIGHT_CENTER,
            format!("{cpu}%"),
            mono.clone(),
            if k == 1 { fg } else { theme::dim() },
        );
    }
    // The status bar, with the badge and the clock.
    let bar = Rect::from_min_max(pos2(r.left() + 1.0, r.bottom() - 26.0), pos2(r.right() - 1.0, r.bottom() - 1.0));
    p.rect_filled(bar, egui::CornerRadius { nw: 0, ne: 0, sw: 7, se: 7 }, theme::bar());
    let badge = Rect::from_min_size(pos2(bar.left() + 8.0, bar.center().y - 9.0), vec2(64.0, 18.0));
    p.rect_filled(badge, 9.0, theme::blue());
    p.text(badge.center(), Align2::CENTER_CENTER, tr("mode.normal"), mono.clone(), theme::bg());
    let now = chrono::Local::now().naive_local();
    let clock = Clock::default().text(&now, current_language());
    p.text(pos2(bar.right() - 10.0, bar.center().y), Align2::RIGHT_CENTER, clock, mono, theme::strong());
}
