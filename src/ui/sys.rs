//! Sys (0): what does not change while running. The machine, its processor,
//! firmware, board and memory on the left, with the GPUs and drives and the
//! keys of their screens; the memory slots in the middle; the
//! selected slot's module on the right. Read once at startup, on a thread of
//! its own (`collect/hw`).

use egui::{Color32, Rect, pos2, vec2};

use super::{Cx, GAP, gpu, gpu::para, kv_rows, now_secs, panel, row_at};
use crate::app::App;
use crate::collect::disk::{Drive, Kind};
use crate::collect::hw::{Hardware, Slot};
use crate::config::cmd::{Act, Goto};
use crate::config::keys::render_seq;
use crate::config::theme::{self, ROW};
use crate::i18n::{tr, trf};
use crate::remote::Link;
use crate::remote::client::Why;
use crate::remote::proto::Hello;
use crate::screen::{Pane, Screen};
use crate::ui::fleet::why_text;
use crate::util::{fmt_bytes, fmt_epoch, fmt_uptime};

pub fn draw(cx: &mut Cx, [left, mid, right]: [Rect; 3]) {
    draw_left(cx, left);
    draw_mid(cx, mid);
    draw_right(cx, right);
}

/// A row of the left column: a section's header or a fact.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Head(&'static str),
    Kv(&'static str, String),
}

/// The left column's rows. Facts the firmware leaves blank are left out, and
/// a section with none at all with them.
pub fn lines(app: &App) -> Vec<Line> {
    let m = &app.model;
    let mut out = Vec::new();
    let mut section = |head: &'static str, rows: Vec<(&'static str, String)>| {
        let rows: Vec<Line> = rows.into_iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| Line::Kv(k, v)).collect();
        if !rows.is_empty() {
            out.push(Line::Head(head));
            out.extend(rows);
        }
    };
    let none = Hardware::default();
    let hw = m.hw.as_ref().unwrap_or(&none);
    let s = &hw.system;
    let mut system = vec![(tr("sys.maker"), s.vendor.clone()), (tr("sys.model"), s.name.clone()), (tr("sys.version"), s.version.clone())];
    // Lenovo writes the same name as the version and the family.
    if s.family != s.version {
        system.push((tr("sys.family"), s.family.clone()));
    }
    system.push((tr("sys.sku"), s.sku.clone()));
    if let Some(st) = &m.st {
        system.extend([
            (tr("sys.host"), st.host.clone()),
            (tr("sys.os"), st.os.clone()),
            (tr("sys.kernel"), st.kernel.clone()),
            (tr("sys.arch"), st.arch.clone()),
            (tr("sys.uptime"), fmt_uptime(now_secs().saturating_sub(st.boot_time))),
            (tr("sys.booted"), trf("sys.utc", &[&fmt_epoch(st.boot_time)])),
        ]);
    }
    section(tr("sys.system"), system);
    section(tr("sys.processor"), processor(app, hw));
    section(tr("sys.agent"), agent(app));
    if m.hw.is_none() {
        out.push(Line::Head(tr("misc.reading")));
        return out;
    }
    let b = &hw.bios;
    let boot = match b.uefi {
        Some(true) => tr("sys.uefi"),
        Some(false) => tr("sys.legacy"),
        None => "",
    };
    section(
        tr("sys.firmware"),
        vec![
            (tr("sys.maker"), b.vendor.clone()),
            (tr("sys.version"), b.version.clone()),
            (tr("sys.date"), b.date.clone()),
            (tr("sys.release"), b.release.clone()),
            (tr("sys.boot"), boot.to_string()),
            (tr("sys.vm"), if b.vm { tr("sys.yes").to_string() } else { String::new() }),
            (tr("sys.source"), hw.source.clone()),
        ],
    );
    let d = &hw.board;
    section(
        tr("sys.board"),
        vec![(tr("sys.maker"), d.vendor.clone()), (tr("sys.model"), d.name.clone()), (tr("sys.version"), d.version.clone())],
    );
    let installed = match hw.installed() {
        Some(b) => (tr("sys.installed"), fmt_bytes(b)),
        None if m.mem.total > 0 => (tr("sys.total"), fmt_bytes(m.mem.total)),
        None => (tr("sys.installed"), String::new()),
    };
    let slots = if hw.unified || hw.slots_total() == 0 {
        String::new()
    } else {
        trf("sys.slots_used", &[&hw.used().to_string(), &hw.slots_total().to_string()])
    };
    section(
        tr("sys.memory"),
        vec![
            installed,
            (tr("sys.slots"), slots),
            (tr("sys.max"), if hw.max_mem > 0 { fmt_bytes(hw.max_mem) } else { String::new() }),
            (tr("sys.error_check"), hw.ecc.map(|e| tr(e.key()).to_string()).unwrap_or_default()),
        ],
    );
    let jump = |to: Screen| (tr("sys.details"), screen_keys(app, to).map(|k| trf("sys.press", &[&k])).unwrap_or_default());
    let with = |name: String, size: Option<u64>| match size {
        Some(b) if b > 0 => format!("{name} · {}", fmt_bytes(b)),
        _ => name,
    };
    let mut graphics: Vec<_> = m.gpus.iter().map(|d| (gpu::kind_label(d.kind), with(gpu::name(d), d.vram_total))).collect();
    if !graphics.is_empty() {
        graphics.push(jump(Screen::Gpu));
    }
    section(tr("sys.graphics"), graphics);
    let mut drives: Vec<_> = m
        .disks
        .drives
        .iter()
        .map(|d| (drive_kind(d), with(if d.model.is_empty() { d.name.clone() } else { d.model.clone() }, d.size)))
        .collect();
    if !drives.is_empty() {
        drives.push(jump(Screen::Disk));
    }
    section(tr("sys.drives"), drives);
    out
}

/// SSD or HDD, else the bus it hangs on, else just a drive.
fn drive_kind(d: &Drive) -> &'static str {
    match (d.kind, d.bus) {
        (Kind::Unknown, Some(bus)) => bus,
        (Kind::Unknown, None) => tr("sys.drive"),
        (k, _) => tr(k.key()),
    }
}

/// The main keys that open `to`, the way help joins them; none when unbound.
fn screen_keys(app: &App, to: Screen) -> Option<String> {
    let act = Act::Screen(Goto::To(to));
    let keys: Vec<String> =
        app.keymap().main.iter().filter(|b| b.run.as_slice() == std::slice::from_ref(&act)).map(|b| render_seq(&b.on)).collect();
    (!keys.is_empty()).then(|| keys.join(" / "))
}

/// The processor's rows: the model as the OS names it, the rest from the
/// firmware, the counts from the topology where the firmware says none.
fn processor(app: &App, hw: &Hardware) -> Vec<(&'static str, String)> {
    let st = app.model.st.as_ref();
    let first = hw.cpus.first();
    let name = st.map(|st| st.brand.clone()).filter(|b| !b.is_empty()).or_else(|| first.map(|c| c.name.clone())).unwrap_or_default();
    let socket = first.map(|c| c.socket.clone()).unwrap_or_default();
    let socket = match hw.cpus.len() {
        n if n > 1 && socket.is_empty() => n.to_string(),
        n if n > 1 => trf("sys.sockets", &[&n.to_string(), &socket]),
        _ => socket,
    };
    let (mut cores, mut threads) = hw.cpu_counts();
    // A topology made up of one core per logical CPU says nothing of cores.
    if let Some(t) = st.map(|st| &st.topo) {
        if cores == 0 && t.source != "fallback" {
            cores = t.core_count() as u32;
        }
        if threads == 0 {
            threads = t.logical as u32;
        }
    }
    let n = |v: u32| if v > 0 { v.to_string() } else { String::new() };
    let cache = |i: usize| first.map(|c| c.cache[i]).filter(|&b| b > 0).map(fmt_bytes).unwrap_or_default();
    vec![
        (tr("sys.model"), name),
        (tr("sys.maker"), first.map(|c| c.vendor.clone()).unwrap_or_default()),
        (tr("sys.socket"), socket),
        (tr("sys.cores"), n(cores)),
        (tr("sys.threads"), n(threads)),
        (tr("sys.max_speed"), first.map(|c| c.max_mhz).filter(|&m| m > 0).map(|m| trf("sys.mhz", &[&m.to_string()])).unwrap_or_default()),
        (tr("sys.l1"), cache(0)),
        (tr("sys.l2"), cache(1)),
        (tr("sys.l3"), cache(2)),
    ]
}

/// The fleet agent of the machine whose screens show: the one this window
/// is inside, else what the last look on this machine's port found.
fn agent(app: &App) -> Vec<(&'static str, String)> {
    let allow = |a: &[String]| if a.is_empty() { tr("fleet.view_only").to_string() } else { a.join(", ") };
    let up = |h: &Hello, viewers: Option<u16>| {
        vec![
            (tr("sys.agent_state"), trf("sys.agent_listening", &[&format!("127.0.0.1:{}", h.port)])),
            (tr("sys.version"), h.version.clone()),
            (tr("sys.agent_pid"), h.pid.to_string()),
            (tr("sys.agent_user"), h.user.clone()),
            (tr("sys.agent_allow"), allow(&h.allow)),
            (tr("sys.agent_interval"), trf("settings.seconds", &[&h.interval.to_string()])),
            (tr("sys.agent_viewers"), viewers.map(|v| v.to_string()).unwrap_or_default()),
        ]
    };
    if let Some(p) = app.away_peer() {
        return match (&p.link, &p.hello) {
            (Link::Up, Some(h)) => up(h, p.summary.as_ref().map(|s| s.viewers)),
            (Link::Down(why), _) => vec![(tr("sys.agent_state"), why_text(why))],
            _ => vec![(tr("sys.agent_state"), tr("fleet.connecting").to_string())],
        };
    }
    let settings = &app.prefs.settings;
    let off = |state: String| {
        vec![
            (tr("sys.agent_state"), state),
            (tr("sys.agent_port"), settings.port.to_string()),
            (tr("sys.agent_allow"), allow(&settings.allow)),
            (tr("sys.agent_start"), tr("sys.agent_start_how").to_string()),
        ]
    };
    match &app.probe.last {
        Some(Ok((h, sum))) => up(h, Some(sum.viewers)),
        Some(Err(Why::Refused)) => off(tr("sys.agent_off").to_string()),
        Some(Err(why)) => off(why_text(why)),
        None => vec![(tr("sys.agent_state"), tr("sys.agent_looking").to_string())],
    }
}

/// The machine in a few words: its maker and model, else the host's name.
pub fn machine(app: &App) -> String {
    let m = &app.model;
    let host = || m.st.as_ref().map(|st| st.host.clone()).unwrap_or_default();
    let Some(hw) = &m.hw else { return host() };
    // A self-built PC leaves the system blank; its board names it better.
    let part = if hw.system.name.is_empty() && !hw.board.name.is_empty() { &hw.board } else { &hw.system };
    let (v, n) = (&part.vendor, &part.name);
    match (v.is_empty(), n.is_empty()) {
        (_, true) if part.version.is_empty() => host(),
        // The model alone when the maker is already in its name.
        (false, false) if !n.to_lowercase().starts_with(&v.to_lowercase()) => format!("{v} {n}"),
        (_, false) => n.clone(),
        (_, true) => format!("{v} {}", part.version).trim().to_string(),
    }
}

fn draw_left(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let lines = lines(app);
    let body = panel(&cx.pen, r, tr("sys.machine"), "", cx.focused(Pane::Left));
    let longest = lines.iter().map(|l| if let Line::Kv(k, _) = l { cx.pen.width(k) } else { 0.0 }).fold(0.0, f32::max);
    let kw = (longest + cx.pen.cw * 2.0).clamp(cx.pen.cw * 6.0, (body.width() * 0.5).max(cx.pen.cw * 6.0));
    cx.scroll_list(Pane::Left, body, lines.len(), |cx, row, i| match &lines[i] {
        Line::Head(h) => cx.pen.section(row, h),
        Line::Kv(k, v) => cx.pen.kv(row, kw, k, v),
    });
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Col {
    Slot,
    Size,
    Kind,
    Speed,
    Maker,
}

/// Width in characters (0 shares what is left) and whether it is right-aligned.
const COLS: [(Col, usize, bool); 5] =
    [(Col::Slot, 0, false), (Col::Size, 9, true), (Col::Kind, 7, false), (Col::Speed, 10, true), (Col::Maker, 0, false)];

/// The columns that fit in `w` pixels: the maker, then the type, go while
/// the slot's name would be under 10 characters.
fn columns(w: f32, cw: f32) -> Vec<(Col, f32, f32, bool)> {
    let mut cols = COLS.to_vec();
    let flex = |c: &[(Col, usize, bool)]| {
        let fixed = c.iter().map(|&(_, n, _)| n + 1).sum::<usize>() as f32 * cw;
        (w - fixed) / c.iter().filter(|&&(_, n, _)| n == 0).count() as f32
    };
    for drop in [Col::Maker, Col::Kind] {
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
        Col::Slot => tr("sys.col.slot"),
        Col::Size => tr("sys.col.size"),
        Col::Kind => tr("sys.col.type"),
        Col::Speed => tr("sys.col.speed"),
        Col::Maker => tr("sys.col.maker"),
    }
}

/// Where the slot is; unified memory has no slot to name.
fn place(s: &Slot, unified: bool) -> String {
    if s.locator.is_empty() && unified { tr("sys.in_package").to_string() } else { s.locator.clone() }
}

/// MT/s, as it runs if that is said, else as rated.
fn speed(s: &Slot) -> String {
    match if s.configured > 0 { s.configured } else { s.speed } {
        0 => String::new(),
        n => trf("sys.mts", &[&n.to_string()]),
    }
}

fn cell(s: &Slot, unified: bool, col: Col) -> (String, Color32) {
    if !s.present {
        return match col {
            Col::Slot => (place(s, unified), theme::dim()),
            Col::Size => (tr("sys.empty").to_string(), theme::dim()),
            _ => (String::new(), theme::dim()),
        };
    }
    match col {
        Col::Slot => (place(s, unified), theme::fg()),
        Col::Size => (s.size.map_or_else(|| "—".to_string(), fmt_bytes), theme::cyan()),
        Col::Kind => (s.kind.clone(), theme::fg()),
        Col::Speed => (speed(s), theme::fg()),
        Col::Maker => (s.vendor.clone(), theme::dim()),
    }
}

fn draw_mid(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let Some(hw) = &app.model.hw else {
        let body = panel(&cx.pen, r, tr("sys.slot_list"), "", cx.focused(Pane::Mid));
        cx.pen.text(pos2(body.left() + 4.0, body.top() + ROW / 2.0), tr("misc.reading"), theme::dim());
        return;
    };
    let info = match hw.installed() {
        _ if hw.unified || hw.slots.is_empty() => String::new(),
        Some(b) => trf("sys.slot_info", &[&hw.used().to_string(), &hw.slots_total().to_string(), &fmt_bytes(b)]),
        None => trf("sys.slots_used", &[&hw.used().to_string(), &hw.slots_total().to_string()]),
    };
    let body = panel(&cx.pen, r, tr("sys.slot_list"), &info, cx.focused(Pane::Mid));
    if hw.slots.is_empty() {
        let y = para(&cx.pen, body, body.top(), tr("sys.no_slots"), theme::dim()) + GAP;
        if let Some(n) = hw.note {
            para(&cx.pen, body, y, tr(n.key()), theme::yellow());
        }
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
    let list = Rect::from_min_size(rows.min, vec2(rows.width(), (hw.slots.len() as f32 * ROW).min(rows.height())));
    cx.scroll_list(Pane::Mid, list, hw.slots.len(), |cx, row, i| {
        let s = &hw.slots[i];
        let cy = row.center().y;
        for &(col, x, w, right) in &cols {
            let (text, color) = cell(s, hw.unified, col);
            let text = cx.pen.fit(&text, w);
            if right {
                cx.pen.text_r(pos2(x0 + x + w, cy), &text, color);
            } else {
                cx.pen.text(pos2(x0 + x, cy), &text, color);
            }
        }
    });
    if hw.unified {
        para(&cx.pen, rows, list.bottom() + GAP, tr("sys.unified"), theme::dim());
    }
}

/// The selected slot's facts; those not said are left out.
fn detail_rows(s: &Slot, unified: bool) -> Vec<(&'static str, String)> {
    let n = |v: u32| if v > 0 { trf("sys.mts", &[&v.to_string()]) } else { String::new() };
    let state = if s.present { tr("sys.present") } else { tr("sys.empty") };
    let rows = vec![
        (tr("sys.slot"), place(s, unified)),
        (tr("sys.bank"), s.bank.clone()),
        (tr("sys.state"), state.to_string()),
        (tr("sys.size"), s.size.map(fmt_bytes).unwrap_or_default()),
        (tr("sys.type"), s.kind.clone()),
        (tr("sys.form"), s.form.clone()),
        (tr("sys.speed"), n(s.speed)),
        (tr("sys.configured"), n(s.configured)),
        (tr("sys.rank"), if s.rank > 0 { s.rank.to_string() } else { String::new() }),
        (tr("sys.voltage"), if s.mv > 0 { format!("{:.2} V", f32::from(s.mv) / 1000.0) } else { String::new() }),
        (tr("sys.maker"), s.vendor.clone()),
        (tr("sys.part"), s.part.clone()),
    ];
    rows.into_iter().filter(|(_, v)| !v.is_empty()).collect()
}

fn draw_right(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let hw = app.model.hw.as_ref();
    let Some(s) = app.selected_slot() else {
        let body = panel(&cx.pen, r, tr("sys.slot"), "", cx.focused(Pane::Right));
        // Why there are no slots is said once, in the middle.
        if hw.is_some_and(|h| !h.slots.is_empty()) {
            para(&cx.pen, body, body.top(), tr("sys.none"), theme::dim());
        }
        return;
    };
    let unified = hw.is_some_and(|h| h.unified);
    let body = panel(&cx.pen, r, &place(s, unified), &s.bank, cx.focused(Pane::Right));
    kv_rows(&cx.pen, body, body.top(), &detail_rows(s, unified));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect::hw::{Ecc, Note};

    #[test]
    fn narrow_lists_drop_the_maker_then_the_type() {
        let names = |w: f32| columns(w, 1.0).into_iter().map(|(c, ..)| c).collect::<Vec<_>>();
        assert_eq!(names(80.0).len(), 5);
        assert_eq!(names(50.0), [Col::Slot, Col::Size, Col::Kind, Col::Speed]);
        assert_eq!(names(35.0), [Col::Slot, Col::Size, Col::Speed]);
        // The slot and the maker share what is left.
        let cols = columns(80.0, 1.0);
        assert_eq!(cols[0].2, cols[4].2);
    }

    #[test]
    fn a_slot_says_only_what_is_known() {
        let s = Slot {
            locator: "DIMM_A2".into(),
            bank: "BANK 1".into(),
            present: true,
            size: Some(16 << 30),
            kind: "DDR5".into(),
            speed: 4800,
            configured: 6000,
            mv: 1350,
            ..Slot::default()
        };
        let rows = detail_rows(&s, false);
        let keys: Vec<&str> = rows.iter().map(|(k, _)| *k).collect();
        assert_eq!(
            keys,
            [
                tr("sys.slot"),
                tr("sys.bank"),
                tr("sys.state"),
                tr("sys.size"),
                tr("sys.type"),
                tr("sys.speed"),
                tr("sys.configured"),
                tr("sys.voltage")
            ]
        );
        assert_eq!(rows[7].1, "1.35 V");
        assert_eq!(speed(&s), trf("sys.mts", &["6000"]));
        // An empty slot is named and dim; unified memory has no name of its own.
        assert_eq!(
            cell(&Slot { locator: "DIMM_B1".into(), ..Slot::default() }, false, Col::Size),
            (tr("sys.empty").to_string(), theme::dim())
        );
        assert_eq!(place(&Slot::default(), true), tr("sys.in_package"));
    }

    #[test]
    fn every_ecc_and_note_has_english() {
        for e in Ecc::ALL {
            assert_ne!(tr(e.key()), e.key());
        }
        for n in Note::ALL {
            assert_ne!(tr(n.key()), n.key());
        }
    }
}
