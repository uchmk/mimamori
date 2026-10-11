//! The fleet (F): a card for this machine and one for every `[[host]]` in
//! config.toml, over the screens. Each card has a dot for its link, the last
//! minute of CPU, memory and GPU, and a few facts; a card that is down keeps
//! its last numbers, faded, with the reason under them.

use std::time::{Duration, Instant};

use egui::{Color32, Id, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use super::{Cx, Pen, graph, now_secs, panel};
use crate::app::App;
use crate::collect::gpu;
use crate::config::cmd::Act;
use crate::config::keys::render_seq;
use crate::config::theme::{self, ROW, ramp};
use crate::i18n::{tr, trf};
use crate::model::Ring;
use crate::remote::client::Why;
use crate::remote::proto::{Battery, PROTO, Summary, Warn};
use crate::remote::{self, Link, Peer, Status, agent};
use crate::util::fmt_uptime;

/// The narrowest a card gets before the row takes one fewer.
const CARD_MIN: f32 = 300.0;
const CARD_GAP: f32 = 8.0;
const PAD: f32 = 10.0;
/// Header, system, three graphs, facts and the last line.
const LINES: usize = 7;

fn card_h() -> f32 {
    PAD * 2.0 + ROW * LINES as f32
}

/// Cards in a row of `w` pixels.
pub fn columns(w: f32) -> usize {
    (((w + CARD_GAP) / (CARD_MIN + CARD_GAP)).floor() as usize).max(1)
}

pub fn dot(s: Status) -> Color32 {
    match s {
        Status::Ok => theme::green(),
        Status::Slow => theme::yellow(),
        Status::Down => theme::red(),
        Status::Connecting => theme::dim(),
    }
}

/// Why a host is down, said on its card.
pub fn why_text(w: &Why) -> String {
    match w {
        Why::Refused => tr("fleet.why.refused").into(),
        Why::Unknown => tr("fleet.why.unknown").into(),
        Why::Io(e) => trf("fleet.why.io", &[e]),
        Why::Quiet => tr("fleet.why.quiet").into(),
        Why::Closed => tr("fleet.why.closed").into(),
        // The older side is the one to update.
        Why::Proto(p) => trf(if *p < PROTO { "fleet.why.proto_old" } else { "fleet.why.proto_new" }, &[&p.to_string(), &PROTO.to_string()]),
        Why::NotAgent => tr("fleet.why.not_agent").into(),
        Why::Denied => tr("fleet.why.denied").into(),
        Why::BadKey => tr("fleet.why.bad_key").into(),
    }
}

/// One graph row of a card: its history and the latest value.
struct Gauge<'a> {
    label: &'static str,
    ring: Option<&'a Ring>,
    value: Option<f32>,
    color: Color32,
}

/// What a card shows, from this machine's model or a peer.
struct Card<'a> {
    name: &'a str,
    addr: &'a str,
    dot: Color32,
    right: String,
    right_c: Color32,
    system: String,
    gauges: [Gauge<'a>; 3],
    /// Name, value and the value's colour.
    facts: Vec<(&'static str, String, Color32)>,
    last: String,
    last_c: Color32,
    faded: bool,
}

pub fn draw(cx: &mut Cx, area: Rect) {
    let app = cx.app;
    let now = Instant::now();
    let up = app.peers.iter().filter(|p| matches!(p.status(now), Status::Ok | Status::Slow)).count() + 1;
    let info = trf("crumb.fleet", &[&(app.peers.len() + 1).to_string(), &up.to_string()]);
    let info = info.split(" · ").take(2).collect::<Vec<_>>().join(" · ");
    let body = panel(&cx.pen, area, tr("tab.fleet"), &info, true);
    let cols = columns(body.width());
    cx.out.fleet_cols = Some(cols);
    let w = (body.width() - CARD_GAP * (cols - 1) as f32) / cols as f32;
    let h = card_h();

    let mut cards = Vec::with_capacity(app.peers.len() + 1);
    cards.push(this_machine(app));
    cards.extend(app.peers.iter().map(|p| peer(p, now)));
    // The card of the machine whose screens show says so.
    let here = app.away.as_ref().map_or(0, |a| a.peer + 1);
    if let Some(c) = cards.get_mut(here) {
        c.right = [tr("fleet.here"), c.right.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
        c.right_c = theme::cyan();
    }

    // Whole rows, scrolled so the selected card shows; the hint takes a row
    // of its own under the cards when there are no hosts.
    let rows_fit = (((body.height() + CARD_GAP) / (h + CARD_GAP)).floor() as usize).max(1);
    let sel_row = app.fleet_sel / cols;
    let first = sel_row.saturating_sub(rows_fit - 1);
    for (i, card) in cards.iter().enumerate() {
        let row = i / cols;
        if row < first || row >= first + rows_fit {
            continue;
        }
        let x = body.left() + (i % cols) as f32 * (w + CARD_GAP);
        let y = body.top() + (row - first) as f32 * (h + CARD_GAP);
        let r = Rect::from_min_size(pos2(x, y), vec2(w, h.min(body.bottom() - y)));
        draw_card(&cx.pen.clipped(r), r, card, i == app.fleet_sel);
        let resp = cx.ui.interact(r, Id::new(("fleet", i)), Sense::click());
        if resp.clicked() {
            cx.out.fleet_click = Some((i, resp.double_clicked()));
        }
    }
    if app.peers.is_empty() {
        let y = body.top() + (cards.len().div_ceil(cols) - first) as f32 * (h + CARD_GAP);
        if y + ROW * 2.0 < body.bottom() {
            let tall = PAD * 2.0 + ROW * hint_lines(app).len() as f32;
            let r = Rect::from_min_max(pos2(body.left(), y), pos2(body.right(), (y + tall).min(body.bottom())));
            hint(cx, r);
        }
    }
    // A host goes slow when its summaries stop, with nothing to wake the
    // window: look again while any is up.
    if app.peers.iter().any(|p| p.link == Link::Up) {
        cx.ui.ctx().request_repaint_after(Duration::from_secs(1));
    }
}

fn this_machine(app: &App) -> Card<'_> {
    let m = app.home();
    let st = m.st.as_ref();
    let sum = agent::summary(m);
    // The GPU the summary names, by its history.
    let gpu_ring =
        m.gpus.iter().find(|d| d.kind == gpu::Kind::Gpu && d.usage.is_some()).and_then(|d| m.gpu_hist.get(&d.id)).map(|(u, _)| u);
    let mut facts = sum_facts(&sum);
    if let Some(st) = st {
        facts.push((tr("fleet.up"), fmt_uptime(now_secs().saturating_sub(st.boot_time)), theme::fg()));
    }
    Card {
        name: st.map_or("", |s| s.host.as_str()),
        addr: "",
        dot: theme::green(),
        right: tr("fleet.this").into(),
        right_c: theme::dim(),
        system: st
            .map(|s| {
                [s.os.as_str(), s.arch.as_str(), s.brand.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")
            })
            .unwrap_or_default(),
        gauges: [
            Gauge { label: tr("fleet.cpu"), ring: Some(&m.cpu_hist), value: Some(sum.cpu), color: theme::blue() },
            Gauge { label: tr("fleet.mem"), ring: Some(&m.mem_hist), value: Some(sum.mem_pct()), color: theme::green() },
            Gauge { label: tr("fleet.gpu"), ring: gpu_ring, value: sum.gpu.map(|g| g.1), color: theme::magenta() },
        ],
        facts,
        last: format!("yagura {}", env!("CARGO_PKG_VERSION")),
        last_c: theme::dim(),
        faded: false,
    }
}

fn peer(p: &Peer, now: Instant) -> Card<'_> {
    let status = p.status(now);
    let sum = p.summary.as_ref();
    let hello = p.hello.as_ref();
    let right = match (&p.link, p.latency) {
        (Link::Connecting, _) => tr("fleet.connecting").to_string(),
        (Link::Up, Some(l)) => trf("fleet.latency", &[&l.as_millis().to_string()]),
        _ => String::new(),
    };
    let mut facts = sum.map(sum_facts).unwrap_or_default();
    if let Some(h) = hello.filter(|h| h.boot_time > 0 && p.link == Link::Up) {
        facts.push((tr("fleet.up"), fmt_uptime(now_secs().saturating_sub(h.boot_time)), theme::fg()));
    }
    if status != Status::Ok
        && let Some(seen) = p.seen.and_then(|t| t.elapsed().ok())
    {
        facts.push((tr("fleet.seen"), fmt_uptime(seen.as_secs()), theme::fg()));
    }
    let (last, last_c) = match &p.link {
        Link::Down(why) => (why_text(why), theme::red()),
        _ => match hello {
            Some(h) if h.allow.is_empty() => (format!("{}: {}", tr("fleet.actions"), tr("fleet.view_only")), theme::dim()),
            Some(h) => (format!("{}: {}", tr("fleet.actions"), h.allow.join(", ")), theme::yellow()),
            None => (String::new(), theme::dim()),
        },
    };
    let system = hello
        .map(|h| [h.os.as_str(), h.arch.as_str(), h.brand.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · "))
        .unwrap_or_default();
    Card {
        name: &p.name,
        addr: &p.addr,
        dot: dot(status),
        right_c: if status == Status::Slow { theme::yellow() } else { theme::dim() },
        right,
        system,
        gauges: [
            Gauge { label: tr("fleet.cpu"), ring: Some(&p.cpu), value: sum.map(|s| s.cpu), color: theme::blue() },
            Gauge { label: tr("fleet.mem"), ring: Some(&p.mem), value: sum.map(|s| s.mem_pct()), color: theme::green() },
            Gauge {
                label: tr("fleet.gpu"),
                ring: sum.and_then(|s| s.gpu.as_ref()).map(|_| &p.gpu),
                value: sum.and_then(|s| s.gpu.as_ref()).map(|g| g.1),
                color: theme::magenta(),
            },
        ],
        facts,
        last,
        last_c,
        faded: status == Status::Down,
    }
}

/// The facts a summary gives: what warns first, then the hottest
/// temperature and the battery.
fn sum_facts(s: &Summary) -> Vec<(&'static str, String, Color32)> {
    let mut facts = Vec::new();
    if let Some((text, red)) = warn_text(&s.warn) {
        facts.push((tr("fleet.warn"), text, if red { theme::red() } else { theme::yellow() }));
    }
    if let Some(t) = s.temp {
        facts.push((tr("fleet.temp"), format!("{t:.0}°C"), theme::fg()));
    }
    if let Some(b) = &s.battery {
        facts.push((tr("fleet.battery"), battery_text(b), if b.pct < 15.0 && !b.plugged { theme::yellow() } else { theme::fg() }));
    }
    facts
}

/// The counts that are not zero, and whether a service failed (red); `None`
/// when nothing warns.
pub fn warn_text(w: &Warn) -> Option<(String, bool)> {
    let failed = w.failed.unwrap_or(0);
    let parts: Vec<String> = [("fleet.warn_hot", w.hot), ("fleet.warn_drives", w.drives), ("fleet.warn_failed", failed)]
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .map(|(k, n)| trf(k, &[&n.to_string()]))
        .collect();
    (w.total() > 0).then(|| (parts.join(" · "), failed > 0))
}

/// `87%`, and whether it charges or is plugged in and full.
pub fn battery_text(b: &Battery) -> String {
    let pct = format!("{:.0}%", b.pct);
    match (b.charging, b.plugged) {
        (true, _) => trf("fleet.battery_charging", &[&pct]),
        (false, true) => trf("fleet.battery_plugged", &[&pct]),
        _ => pct,
    }
}

fn draw_card(pen: &Pen, r: Rect, c: &Card, selected: bool) {
    let fade = |col: Color32| if c.faded { col.gamma_multiply(0.55) } else { col };
    pen.p.rect_filled(r, 6.0, theme::bg());
    pen.p.rect_stroke(r, 6.0, Stroke::new(1.0, if selected { theme::cyan() } else { theme::line() }), StrokeKind::Inside);
    let inner = r.shrink(PAD);
    let line = |k: usize| Rect::from_min_size(pos2(inner.left(), inner.top() + k as f32 * ROW), vec2(inner.width(), ROW));

    // The dot, the name and its address; the latency on the right.
    let head = line(0);
    let y = head.center().y;
    pen.p.circle_filled(pos2(head.left() + 4.0, y), 4.0, c.dot);
    let rr = pen.text_r(pos2(head.right(), y), &c.right, fade(c.right_c));
    let room = rr.left() - head.left() - 14.0 - pen.cw;
    let n = pen.text(pos2(head.left() + 14.0, y), &pen.fit(c.name, room), fade(theme::fg()));
    // Bold by drawing the name again a pixel over.
    pen.text(pos2(head.left() + 15.0, y), &pen.fit(c.name, room), fade(theme::fg()));
    if !c.addr.is_empty() && c.addr != c.name {
        let room = rr.left() - n.right() - pen.cw * 2.0;
        pen.text(pos2(n.right() + pen.cw, y), &pen.fit(c.addr, room), theme::dim().gamma_multiply(0.8));
    }
    let sys = line(1);
    pen.text(pos2(sys.left(), sys.center().y), &pen.fit(&c.system, sys.width()), theme::dim());

    for (k, g) in c.gauges.iter().enumerate() {
        let row = line(2 + k);
        let y = row.center().y;
        pen.text(pos2(row.left(), y), g.label, theme::dim());
        let value = g.value.map_or_else(|| "—".to_string(), |v| format!("{v:.0}%"));
        pen.text_r(pos2(row.right(), y), &value, fade(g.value.map_or(theme::dim(), ramp)));
        let spark =
            Rect::from_min_max(pos2(row.left() + pen.cw * 5.0, row.top() + 4.0), pos2(row.right() - pen.cw * 6.0, row.bottom() - 4.0));
        if let Some(ring) = g.ring {
            graph::spark(pen, spark, ring.iter(), 100.0, fade(g.color));
        }
        pen.p.line_segment([pos2(spark.left(), spark.bottom()), pos2(spark.right(), spark.bottom())], Stroke::new(1.0, theme::line()));
    }

    // A long reason takes the facts' line when there are none, or when the
    // host is down: why it is down matters more than its old facts.
    let (facts, last) = (line(5), line(6));
    let (top, rest) = match c.facts.is_empty() || c.faded {
        true => split(pen, &c.last, last.width()),
        false => ("", c.last.as_str()),
    };
    if top.is_empty() {
        // Dim names, values in the text colour.
        let mut x = facts.left();
        let y = facts.center().y;
        for (k, (name, value, color)) in c.facts.iter().enumerate() {
            if k > 0 {
                x = pen.text(pos2(x, y), " · ", theme::dim()).right();
            }
            x = pen.text(pos2(x, y), &format!("{name} "), theme::dim()).right();
            x = pen.text(pos2(x, y), value, fade(*color)).right();
        }
    } else {
        pen.text(pos2(facts.left(), facts.center().y), top, c.last_c);
    }
    pen.text(pos2(last.left(), last.center().y), &pen.fit(rest, last.width()), c.last_c);
}

/// The words of `s` that fit in `w` and the rest; all of it second when it
/// fits whole.
fn split<'s>(pen: &Pen, s: &'s str, w: f32) -> (&'s str, &'s str) {
    if pen.width(s) <= w {
        return ("", s);
    }
    let mut cut = 0;
    for (i, _) in s.match_indices(' ') {
        if pen.width(&s[..i]) > w {
            break;
        }
        cut = i;
    }
    if cut == 0 { ("", s) } else { (&s[..cut], s[cut..].trim_start()) }
}

/// The lines of the hint: what is said, its colour, and what a click on it
/// copies.
fn hint_lines(app: &App) -> Vec<(String, Color32, Option<String>)> {
    let key = app.keymap().main.iter().find(|b| b.run == [Act::Settings]).map(|b| render_seq(&b.on)).unwrap_or_default();
    let cmd = |s: String| (format!("  {s}"), theme::cyan(), Some(s));
    let mut lines = vec![
        (tr("fleet.no_hosts").into(), theme::yellow(), None),
        (trf("fleet.hint_settings", &[&key]), theme::dim(), None),
        (tr("fleet.hint_agent").into(), theme::dim(), None),
        cmd(remote::AGENT_LINE.into()),
        (tr("fleet.hint_reach").into(), theme::dim(), None),
        cmd(remote::ssh_line(app.prefs.settings.port)),
        cmd(remote::tailscale_line(app.prefs.settings.port)),
        (tr("fleet.hint_add").into(), theme::dim(), None),
    ];
    let sample = remote::host_sample(app.prefs.settings.port);
    lines.extend(sample.lines().map(|l| (format!("  {l}"), theme::cyan(), Some(sample.clone()))));
    if let Some(path) = app.prefs.config_path() {
        let path = path.display().to_string();
        lines.push((format!("  {path}"), theme::dim(), Some(path)));
    }
    lines
}

/// How to add a machine, while there is none; a click on a command copies it.
fn hint(cx: &mut Cx, r: Rect) {
    let pen = cx.pen.clipped(r);
    pen.p.rect_stroke(r, 6.0, Stroke::new(1.0, theme::line()), StrokeKind::Inside);
    let inner = r.shrink(PAD);
    for (k, (s, c, copy)) in hint_lines(cx.app).into_iter().enumerate() {
        let y = inner.top() + (k as f32 + 0.5) * ROW;
        if y > inner.bottom() {
            break;
        }
        let row = Rect::from_center_size(pos2(inner.center().x, y), vec2(inner.width(), ROW));
        let mut color = c;
        if let Some(text) = copy {
            let resp = cx.ui.interact(row, Id::new(("fleet-hint", k)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.hovered() {
                pen.p.rect_filled(row, 3.0, theme::sel_unfocused());
                color = theme::strong();
            }
            if resp.clicked() {
                cx.out.copy = Some(text);
            }
        }
        pen.text(pos2(inner.left(), y), &pen.fit(&s, inner.width()), color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_counts_warnings_and_says_the_battery() {
        assert_eq!(warn_text(&Warn::default()), None);
        // Services not read yet count as none failed.
        assert_eq!(warn_text(&Warn { hot: 0, drives: 0, failed: None }), None);
        let (text, red) = warn_text(&Warn { hot: 2, drives: 0, failed: Some(0) }).unwrap();
        assert!(text.contains('2') && !red, "{text}");
        let (text, red) = warn_text(&Warn { hot: 1, drives: 1, failed: Some(3) }).unwrap();
        assert_eq!(text.matches(" · ").count(), 2, "{text}");
        assert!(red);
        let b = |charging, plugged| Battery { pct: 86.6, charging, plugged };
        assert_eq!(battery_text(&b(false, false)), "87%");
        assert_ne!(battery_text(&b(true, true)), "87%");
        assert_ne!(battery_text(&b(false, true)), battery_text(&b(true, true)));
        assert!(battery_text(&b(false, true)).contains("87%"));
    }
}
