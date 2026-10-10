//! The fleet (F): a card for this machine and one for every `[[host]]` in
//! config.toml, over the screens. Each card has a dot for its link, the last
//! minute of CPU, memory and GPU, and a few facts; a card that is down keeps
//! its last numbers, faded, with the reason under them.

use std::time::{Duration, Instant};

use egui::{Color32, Id, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use super::{Cx, Pen, graph, now_secs, panel};
use crate::app::App;
use crate::collect::gpu;
use crate::config::theme::{BG, BLUE, CYAN, DIM, FG, GREEN, LINE, MAGENTA, RED, ROW, YELLOW, ramp};
use crate::i18n::{tr, trf};
use crate::model::Ring;
use crate::remote::client::Why;
use crate::remote::proto::{PORT, PROTO};
use crate::remote::{Link, Peer, Status, agent};
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
        Status::Ok => GREEN,
        Status::Slow => YELLOW,
        Status::Down => RED,
        Status::Connecting => DIM,
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
        Why::Proto(p) => trf("fleet.why.proto", &[&p.to_string(), &PROTO.to_string()]),
        Why::NotAgent => tr("fleet.why.not_agent").into(),
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
    facts: Vec<(&'static str, String)>,
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
            let r = Rect::from_min_max(pos2(body.left(), y), pos2(body.right(), (y + h).min(body.bottom())));
            hint(&cx.pen.clipped(r), r);
        }
    }
    // A host goes slow when its summaries stop, with nothing to wake the
    // window: look again while any is up.
    if app.peers.iter().any(|p| p.link == Link::Up) {
        cx.ui.ctx().request_repaint_after(Duration::from_secs(1));
    }
}

fn this_machine(app: &App) -> Card<'_> {
    let m = &app.model;
    let st = m.st.as_ref();
    let sum = agent::summary(m);
    // The GPU the summary names, by its history.
    let gpu_ring =
        m.gpus.iter().find(|d| d.kind == gpu::Kind::Gpu && d.usage.is_some()).and_then(|d| m.gpu_hist.get(&d.id)).map(|(u, _)| u);
    let mut facts = Vec::new();
    if let Some(t) = sum.temp {
        facts.push((tr("fleet.temp"), format!("{t:.0}°C")));
    }
    if let Some(st) = st {
        facts.push((tr("fleet.up"), fmt_uptime(now_secs().saturating_sub(st.boot_time))));
    }
    Card {
        name: st.map_or("", |s| s.host.as_str()),
        addr: "",
        dot: GREEN,
        right: tr("fleet.this").into(),
        right_c: DIM,
        system: st
            .map(|s| {
                [s.os.as_str(), s.arch.as_str(), s.brand.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")
            })
            .unwrap_or_default(),
        gauges: [
            Gauge { label: tr("fleet.cpu"), ring: Some(&m.cpu_hist), value: Some(sum.cpu), color: BLUE },
            Gauge { label: tr("fleet.mem"), ring: Some(&m.mem_hist), value: Some(sum.mem_pct()), color: GREEN },
            Gauge { label: tr("fleet.gpu"), ring: gpu_ring, value: sum.gpu.map(|g| g.1), color: MAGENTA },
        ],
        facts,
        last: format!("mimamori {}", env!("CARGO_PKG_VERSION")),
        last_c: DIM,
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
    let mut facts = Vec::new();
    if let Some(t) = sum.and_then(|s| s.temp) {
        facts.push((tr("fleet.temp"), format!("{t:.0}°C")));
    }
    if let Some(h) = hello.filter(|h| h.boot_time > 0 && p.link == Link::Up) {
        facts.push((tr("fleet.up"), fmt_uptime(now_secs().saturating_sub(h.boot_time))));
    }
    if status != Status::Ok
        && let Some(seen) = p.seen.and_then(|t| t.elapsed().ok())
    {
        facts.push((tr("fleet.seen"), fmt_uptime(seen.as_secs())));
    }
    let (last, last_c) = match &p.link {
        Link::Down(why) => (why_text(why), RED),
        _ => match hello {
            Some(h) if h.allow.is_empty() => (format!("{}: {}", tr("fleet.actions"), tr("fleet.view_only")), DIM),
            Some(h) => (format!("{}: {}", tr("fleet.actions"), h.allow.join(", ")), YELLOW),
            None => (String::new(), DIM),
        },
    };
    let system = hello
        .map(|h| [h.os.as_str(), h.arch.as_str(), h.brand.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · "))
        .unwrap_or_default();
    Card {
        name: &p.name,
        addr: &p.addr,
        dot: dot(status),
        right_c: if status == Status::Slow { YELLOW } else { DIM },
        right,
        system,
        gauges: [
            Gauge { label: tr("fleet.cpu"), ring: Some(&p.cpu), value: sum.map(|s| s.cpu), color: BLUE },
            Gauge { label: tr("fleet.mem"), ring: Some(&p.mem), value: sum.map(|s| s.mem_pct()), color: GREEN },
            Gauge {
                label: tr("fleet.gpu"),
                ring: sum.and_then(|s| s.gpu.as_ref()).map(|_| &p.gpu),
                value: sum.and_then(|s| s.gpu.as_ref()).map(|g| g.1),
                color: MAGENTA,
            },
        ],
        facts,
        last,
        last_c,
        faded: status == Status::Down,
    }
}

fn draw_card(pen: &Pen, r: Rect, c: &Card, selected: bool) {
    let fade = |col: Color32| if c.faded { col.gamma_multiply(0.55) } else { col };
    pen.p.rect_filled(r, 6.0, BG);
    pen.p.rect_stroke(r, 6.0, Stroke::new(1.0, if selected { CYAN } else { LINE }), StrokeKind::Inside);
    let inner = r.shrink(PAD);
    let line = |k: usize| Rect::from_min_size(pos2(inner.left(), inner.top() + k as f32 * ROW), vec2(inner.width(), ROW));

    // The dot, the name and its address; the latency on the right.
    let head = line(0);
    let y = head.center().y;
    pen.p.circle_filled(pos2(head.left() + 4.0, y), 4.0, c.dot);
    let rr = pen.text_r(pos2(head.right(), y), &c.right, fade(c.right_c));
    let room = rr.left() - head.left() - 14.0 - pen.cw;
    let n = pen.text(pos2(head.left() + 14.0, y), &pen.fit(c.name, room), fade(FG));
    // Bold by drawing the name again a pixel over.
    pen.text(pos2(head.left() + 15.0, y), &pen.fit(c.name, room), fade(FG));
    if !c.addr.is_empty() && c.addr != c.name {
        let room = rr.left() - n.right() - pen.cw * 2.0;
        pen.text(pos2(n.right() + pen.cw, y), &pen.fit(c.addr, room), DIM.gamma_multiply(0.8));
    }
    let sys = line(1);
    pen.text(pos2(sys.left(), sys.center().y), &pen.fit(&c.system, sys.width()), DIM);

    for (k, g) in c.gauges.iter().enumerate() {
        let row = line(2 + k);
        let y = row.center().y;
        pen.text(pos2(row.left(), y), g.label, DIM);
        let value = g.value.map_or_else(|| "—".to_string(), |v| format!("{v:.0}%"));
        pen.text_r(pos2(row.right(), y), &value, fade(g.value.map_or(DIM, ramp)));
        let spark =
            Rect::from_min_max(pos2(row.left() + pen.cw * 5.0, row.top() + 4.0), pos2(row.right() - pen.cw * 6.0, row.bottom() - 4.0));
        if let Some(ring) = g.ring {
            graph::spark(pen, spark, ring.iter(), 100.0, fade(g.color));
        }
        pen.p.line_segment([pos2(spark.left(), spark.bottom()), pos2(spark.right(), spark.bottom())], Stroke::new(1.0, LINE));
    }

    // Dim names, values in the text colour.
    let facts = line(5);
    let mut x = facts.left();
    let y = facts.center().y;
    for (k, (name, value)) in c.facts.iter().enumerate() {
        if k > 0 {
            x = pen.text(pos2(x, y), " · ", DIM).right();
        }
        x = pen.text(pos2(x, y), &format!("{name} "), DIM).right();
        x = pen.text(pos2(x, y), value, fade(FG)).right();
    }
    // A long reason takes the facts' line when there are none.
    let last = line(6);
    let (top, rest) = match c.facts.is_empty() {
        true => split(pen, &c.last, last.width()),
        false => ("", c.last.as_str()),
    };
    if !top.is_empty() {
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

/// How to add a machine, while there is none.
fn hint(pen: &Pen, r: Rect) {
    pen.p.rect_stroke(r, 6.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
    let inner = r.shrink(PAD);
    let lines: [(String, Color32); 7] = [
        (tr("fleet.no_hosts").into(), YELLOW),
        (tr("fleet.hint_agent").into(), DIM),
        ("  mimamori --agent".into(), CYAN),
        (tr("fleet.hint_reach").into(), DIM),
        (format!("  ssh -L {PORT}:localhost:{PORT} <host>   ·   tailscale serve --bg --tcp {PORT} tcp://localhost:{PORT}"), CYAN),
        (tr("fleet.hint_add").into(), DIM),
        (format!("  [[host]]  name = \"nas\"  addr = \"localhost:{PORT}\""), CYAN),
    ];
    for (k, (s, c)) in lines.iter().enumerate() {
        let y = inner.top() + (k as f32 + 0.5) * ROW;
        if y > inner.bottom() {
            break;
        }
        pen.text(pos2(inner.left(), y), &pen.fit(s, inner.width()), *c);
    }
}
