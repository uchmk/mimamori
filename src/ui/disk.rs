//! Disks (8): the drives on the left, the selected drive's read, write and
//! busy graphs over the mounted volumes in the middle, what the drive is over
//! the selected volume on the right. Enter on a drive jumps to its first
//! volume, and on a volume back to its drive.

use egui::{Color32, Rect, pos2, vec2};
use ito_layout::{Dir, Node};

use super::{Cx, GAP, gpu::para, graph, kv_rows, panel, row_at, shown};
use crate::collect::disk::{self, Drive, Health, Smart, Temp, Volume};
use crate::config::theme::{self, ROW};
use crate::i18n::{tr, trf};
use crate::model::Ring;
use crate::screen::Pane;
use crate::stacks;
use crate::util::fmt_bytes;

/// Below this the graphs do not scale up, so an idle drive stays flat.
const FLOOR: f32 = 64.0 * 1024.0;

pub fn draw(cx: &mut Cx, [left, mid, right]: [Rect; 3]) {
    draw_left(cx, left);
    draw_mid(cx, mid);
    draw_right(cx, right);
}

fn rate(b: f64) -> String {
    format!("{}/s", fmt_bytes(b as u64))
}

/// Green, then yellow over three quarters, red over nine tenths.
fn fill_color(pct: f32) -> Color32 {
    if pct >= 90.0 {
        theme::red()
    } else if pct >= 75.0 {
        theme::yellow()
    } else {
        theme::green()
    }
}

fn draw_left(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let m = &app.model;
    let d = &m.disks;
    let info = if m.disk_read { trf("disk.drive_count", &[&d.drives.len().to_string()]) } else { String::new() };
    let body = panel(&cx.pen, r, tr("disk.drives"), &info, cx.focused(Pane::Left));
    if !m.disk_read {
        cx.pen.text(pos2(body.left() + 4.0, body.top() + ROW / 2.0), tr("misc.reading"), theme::dim());
        return;
    }
    let drives = &d.drives;
    let list = Rect::from_min_size(body.min, vec2(body.width(), (drives.len() as f32 * ROW).min(body.height())));
    cx.scroll_list(Pane::Left, list, drives.len(), |cx, row, i| {
        let x = &drives[i];
        let cy = row.center().y;
        let rr = cx.pen.text_r(pos2(row.right() - 6.0, cy), &rate(x.read_rate + x.write_rate), theme::cyan());
        let room = rr.left() - row.left() - 6.0 - cx.pen.cw * 2.0;
        let name = cx.pen.fit(&x.name, room);
        cx.pen.text(pos2(row.left() + 6.0, cy), &name, if x.health == Some(Health::Failing) { theme::red() } else { theme::fg() });
    });
    // The whole machine, and why the drive list may be short.
    let mut y = list.bottom() + if drives.is_empty() { 0.0 } else { GAP };
    if y + ROW > body.bottom() {
        return;
    }
    cx.pen.section(row_at(body, y), tr("disk.all"));
    y += ROW;
    let (rd, wr) = d.rates();
    let space = |f: fn(&Volume) -> u64| fmt_bytes(d.volumes.iter().map(f).sum());
    let rows = [
        (tr("disk.read"), rate(rd)),
        (tr("disk.write"), rate(wr)),
        (tr("disk.volumes"), d.volumes.len().to_string()),
        (tr("disk.size"), space(|v| v.total)),
        (tr("disk.free"), space(|v| v.avail)),
    ];
    y = kv_rows(&cx.pen, body, y, &rows) + GAP;
    if let Some(n) = d.note {
        para(&cx.pen, body, y, tr(n.key()), if drives.is_empty() { theme::yellow() } else { theme::dim() });
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Col {
    Mount,
    Name,
    Fs,
    Size,
    Used,
    Free,
    Pct,
    Read,
    Write,
}

/// Width in characters (0 shares what is left) and whether it is right-aligned.
const COLS: [(Col, usize, bool); 9] = [
    (Col::Mount, 0, false),
    (Col::Name, 0, false),
    (Col::Fs, 6, false),
    (Col::Size, 9, true),
    (Col::Used, 9, true),
    (Col::Free, 9, true),
    (Col::Pct, 5, true),
    (Col::Read, 10, true),
    (Col::Write, 10, true),
];

/// The columns that fit in `w` pixels: the device, the file system, the
/// used space and the traffic go, in that order, while the mount point would
/// be under 12 characters.
fn columns(w: f32, cw: f32) -> Vec<(Col, f32, f32, bool)> {
    let mut cols = COLS.to_vec();
    let flex = |c: &[(Col, usize, bool)]| {
        let fixed = c.iter().map(|&(_, n, _)| n + 1).sum::<usize>() as f32 * cw;
        (w - fixed) / c.iter().filter(|&&(_, n, _)| n == 0).count() as f32
    };
    for drop in [Col::Name, Col::Fs, Col::Used, Col::Write, Col::Read] {
        if flex(&cols) < cw * 12.0 {
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
        Col::Mount => tr("disk.col.mount"),
        Col::Name => tr("disk.col.device"),
        Col::Fs => tr("disk.col.fs"),
        Col::Size => tr("disk.col.size"),
        Col::Used => tr("disk.col.used"),
        Col::Free => tr("disk.col.free"),
        Col::Pct => tr("disk.col.pct"),
        Col::Read => tr("disk.col.read"),
        Col::Write => tr("disk.col.write"),
    }
}

fn cell(v: &Volume, col: Col) -> (String, Color32) {
    match col {
        Col::Mount => (v.mount.clone(), theme::fg()),
        Col::Name => (v.name.clone(), theme::dim()),
        Col::Fs => (v.fs.clone(), theme::dim()),
        Col::Size => (fmt_bytes(v.total), theme::fg()),
        Col::Used => (fmt_bytes(v.used()), theme::fg()),
        Col::Free => (fmt_bytes(v.avail), theme::fg()),
        Col::Pct => v.use_pct().map_or(("—".to_string(), theme::dim()), |p| (format!("{p:.0}%"), fill_color(p))),
        Col::Read => (rate(v.read_rate), if v.read_rate > 0.0 { theme::cyan() } else { theme::dim() }),
        Col::Write => (rate(v.write_rate), if v.write_rate > 0.0 { theme::magenta() } else { theme::dim() }),
    }
}

/// One traffic graph: its label, the value now, its history and its scale
/// (`None` scales to the history over the floor).
type Graph<'a> = (&'static str, Option<String>, Option<&'a Ring>, Option<f32>, Color32);

fn draw_mid(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let m = &app.model;
    // The traffic stacked over the volumes, each graph the column's width
    // like the CPU and memory screens: read, write and busy for the selected
    // drive, or read and write for the selected volume when there are no
    // drives (macOS, or the counters are off).
    let n: u8 = if m.disks.drives.is_empty() { 2 } else { 3 };
    let rects = cx.stack(Pane::Mid, r, stacks::split(Dir::Down, 0.6, stacks::chain(Dir::Down, 0, n), Node::Leaf(n)));
    // Each way on its own scale, so quiet writes are not flattened by reads.
    let mut list: Vec<Graph> = vec![
        (tr("disk.read"), None, None, None, theme::cyan()),
        (tr("disk.write"), None, None, None, theme::magenta()),
        (tr("disk.busy"), None, None, Some(100.0), theme::yellow()),
    ];
    let mut name = None;
    if let Some(d) = app.selected_drive() {
        name = Some(d.name.as_str());
        let hist = m.drive_hist.get(&d.id);
        list[0].1 = Some(rate(d.read_rate));
        list[0].2 = hist.map(|h| &h.0);
        list[1].1 = Some(rate(d.write_rate));
        list[1].2 = hist.map(|h| &h.1);
        if let Some(b) = d.busy {
            list[2].1 = Some(format!("{b:.0}%"));
            list[2].2 = hist.map(|h| &h.2);
        }
    } else if let Some(v) = app.selected_volume() {
        name = Some(v.mount.as_str());
        let hist = m.vol_hist.get(&v.mount);
        list[0].1 = Some(rate(v.read_rate));
        list[0].2 = hist.map(|h| &h.0);
        list[1].1 = Some(rate(v.write_rate));
        list[1].2 = hist.map(|h| &h.1);
    }
    for (k, (label, now, ring, max, color)) in list.into_iter().take(n as usize).enumerate() {
        if !shown(rects[k]) {
            continue;
        }
        let title = name.map_or_else(|| label.to_string(), |n| format!("{label} · {n}"));
        let body = panel(&cx.pen, rects[k], &title, now.as_deref().unwrap_or(""), false);
        match ring {
            Some(ring) => graph::line(&cx.pen, body, ring.iter(), max.unwrap_or_else(|| ring.max().max(FLOOR)), color),
            None => {
                cx.pen.text(pos2(body.left() + 4.0, body.top() + ROW / 2.0), "—", theme::dim());
            }
        }
    }
    if shown(rects[n as usize]) {
        vols(cx, rects[n as usize]);
    }
}

/// The mounted volumes with their space and traffic.
fn vols(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let m = &app.model;
    let vols = &m.disks.volumes;
    let info = if m.disk_read { trf("disk.vol_count", &[&vols.len().to_string()]) } else { String::new() };
    let body = panel(&cx.pen, r, tr("disk.volumes"), &info, cx.focused(Pane::Mid));
    if !m.disk_read {
        cx.pen.text(pos2(body.left() + 4.0, body.top() + ROW / 2.0), tr("misc.reading"), theme::dim());
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
    let list = Rect::from_min_max(pos2(body.left(), h.bottom()), body.max);
    if vols.is_empty() {
        para(&cx.pen, list, list.top(), tr("disk.no_volume"), theme::dim());
        return;
    }
    cx.scroll_list(Pane::Mid, list, vols.len(), |cx, row, i| {
        let v = &vols[i];
        let cy = row.center().y;
        for &(col, x, w, right) in &cols {
            let (text, color) = cell(v, col);
            let text = cx.pen.fit(&text, w);
            if right {
                cx.pen.text_r(pos2(x0 + x + w, cy), &text, color);
            } else {
                cx.pen.text(pos2(x0 + x, cy), &text, color);
            }
        }
    });
}

/// The health row: what SMART says, or why it says nothing.
fn health(d: &Drive) -> (String, Color32) {
    match (d.health, d.why) {
        (Some(h), _) => (tr(h.key()).to_string(), if h == Health::Failing { theme::red() } else { theme::green() }),
        (None, Some(w)) => (tr(w.key()).to_string(), theme::dim()),
        (None, None) => ("—".to_string(), theme::dim()),
    }
}

/// The temperature row, with the drive's warning mark; yellow from the
/// warning mark, red from the critical one.
fn temp_row(t: &Temp) -> (String, Color32) {
    let now = format!("{:.0}°C", t.value);
    let text = match t.high.or(t.crit) {
        Some(m) => trf("disk.temp_mark", &[&now, &format!("{m:.0}°C")]),
        None => now,
    };
    let color = if t.crit.is_some_and(|c| t.value >= c) {
        theme::red()
    } else if t.high.is_some_and(|h| t.value >= h) {
        theme::yellow()
    } else {
        theme::fg()
    };
    (text, color)
}

/// `1234567` → `1,234,567`.
fn group(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// What SMART counts, the rows the drive gave: the life used (yellow from
/// 80%, red at 100%), the hours powered on, the lifetime writes, and the
/// remapped, pending and media errors (yellow from the first).
fn smart_rows(s: &Smart) -> Vec<(&'static str, String, Color32)> {
    let mut rows = Vec::new();
    if let Some(p) = s.life_used {
        let color = if p >= 100 {
            theme::red()
        } else if p >= disk::WORN {
            theme::yellow()
        } else {
            theme::fg()
        };
        rows.push((tr("disk.life_used"), format!("{p}%"), color));
    }
    if let Some(h) = s.power_on_hours {
        rows.push((tr("disk.power_on"), trf("disk.hours", &[&group(h), &group(h / 24)]), theme::fg()));
    }
    if let Some(b) = s.written {
        rows.push((tr("disk.lifetime_writes"), fmt_bytes(b), theme::fg()));
    }
    for (key, n) in [("disk.remapped", s.remapped), ("disk.pending", s.pending), ("disk.media_errors", s.media_errors)] {
        if let Some(n) = n {
            rows.push((tr(key), group(n), if n > 0 { theme::yellow() } else { theme::fg() }));
        }
    }
    if rows.is_empty()
        && let Some(w) = s.why
    {
        rows.push((tr("disk.smart_counts"), tr(w.key()).to_string(), theme::dim()));
    }
    rows
}

fn drive_rows(d: &Drive) -> Vec<(&'static str, String)> {
    let dash = || "—".to_string();
    let yes_no = |b: bool| tr(if b { "disk.yes" } else { "disk.no" }).to_string();
    let mut rows = vec![
        (tr("disk.model"), if d.model.is_empty() { dash() } else { d.model.clone() }),
        (tr("disk.type"), tr(d.kind.key()).to_string()),
        (tr("disk.bus"), d.bus.map_or_else(dash, str::to_string)),
        (tr("disk.size"), d.size.map_or_else(dash, fmt_bytes)),
        (tr("disk.removable_key"), yes_no(d.removable)),
        (tr("disk.read"), rate(d.read_rate)),
        (tr("disk.write"), rate(d.write_rate)),
    ];
    // Windows' counters keep only the speed, not how much since boot.
    for (key, t) in [("disk.read_boot", d.read_total), ("disk.write_boot", d.write_total)] {
        if let Some(b) = t {
            rows.push((tr(key), fmt_bytes(b)));
        }
    }
    rows.push((tr("disk.volumes"), if d.volumes.is_empty() { dash() } else { d.volumes.join(", ") }));
    rows
}

fn volume_rows(v: &Volume) -> Vec<(&'static str, String)> {
    let dash = || "—".to_string();
    let mut flags = Vec::new();
    if v.read_only {
        flags.push(tr("disk.read_only"));
    }
    if v.removable {
        flags.push(tr("disk.removable"));
    }
    let used = match v.use_pct() {
        Some(p) => format!("{} ({p:.0}%)", fmt_bytes(v.used())),
        None => dash(),
    };
    vec![
        (tr("disk.mount"), v.mount.clone()),
        (tr("disk.device"), if v.name.is_empty() { dash() } else { v.name.clone() }),
        (tr("disk.fs"), if v.fs.is_empty() { dash() } else { v.fs.clone() }),
        (tr("disk.size"), fmt_bytes(v.total)),
        (tr("disk.used"), used),
        (tr("disk.free"), fmt_bytes(v.avail)),
        (tr("disk.type"), tr(v.kind.key()).to_string()),
        (tr("disk.flags"), if flags.is_empty() { dash() } else { flags.join(", ") }),
        (tr("disk.drive"), v.drive.clone().unwrap_or_else(dash)),
    ]
}

fn draw_right(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let m = &app.model;
    let vol = app.selected_volume();
    let Some(d) = app.selected_drive() else {
        // No drives (macOS, or the counters are off): the volume alone.
        match vol {
            Some(v) => volume(cx, r, v, cx.focused(Pane::Right)),
            None => {
                let body = panel(&cx.pen, r, tr("disk.detail"), "", cx.focused(Pane::Right));
                if m.disk_read {
                    cx.pen.text(pos2(body.left() + 4.0, body.top() + ROW / 2.0), tr("disk.none"), theme::dim());
                }
            }
        }
        return;
    };
    // The drive over the selected volume, each with the room its rows want.
    let smart = smart_rows(&d.smart);
    let rows = drive_rows(d);
    let n = 1 + usize::from(d.temp.is_some()) + smart.len() + rows.len();
    let rects = match vol {
        Some(_) => {
            // The frame, the gap the split takes and a little slack, so the last row is not cut.
            let want = 26.0 + 6.0 + GAP + ROW * n as f32 + 2.0;
            let ratio = want.min(r.height() * 0.65) / r.height().max(1.0);
            cx.stack(Pane::Right, r, stacks::split(Dir::Down, ratio, Node::Leaf(0), Node::Leaf(1)))
        }
        None => vec![r, Rect::NOTHING],
    };
    if shown(rects[0]) {
        let body = panel(&cx.pen, rects[0], &d.name, tr(d.kind.key()), cx.focused(Pane::Right));
        let mut y = body.top();
        let (text, color) = health(d);
        cx.pen.pair(row_at(body, y), tr("disk.health_key"), theme::dim(), &text, color);
        y += ROW;
        let temp = d.temp.as_ref().map(temp_row).map(|(t, c)| (tr("disk.temp"), t, c));
        for (label, text, color) in temp.into_iter().chain(smart) {
            if y + ROW > body.bottom() {
                break;
            }
            cx.pen.pair(row_at(body, y), label, theme::dim(), &text, color);
            y += ROW;
        }
        kv_rows(&cx.pen, body, y, &rows);
    }
    if let Some(v) = vol
        && shown(rects[1])
    {
        volume(cx, rects[1], v, false);
    }
}

/// The selected volume: how full it is and what it is.
fn volume(cx: &Cx, r: Rect, v: &Volume, focused: bool) {
    let body = panel(&cx.pen, r, &v.mount, tr(v.kind.key()), focused);
    let mut y = body.top();
    if let Some(p) = v.use_pct() {
        let row = row_at(body, y);
        graph::bar(
            &cx.pen,
            Rect::from_min_max(pos2(row.left() + 4.0, row.top() + 7.0), pos2(row.right() - 4.0, row.bottom() - 7.0)),
            p,
            fill_color(p),
        );
        y += ROW;
    }
    kv_rows(&cx.pen, body, y, &volume_rows(v));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect::disk::Why;

    #[test]
    fn narrow_lists_drop_the_device_fs_used_and_traffic() {
        let names = |w: f32| columns(w, 1.0).into_iter().map(|(c, ..)| c).collect::<Vec<_>>();
        assert_eq!(names(200.0).len(), 9);
        assert_eq!(names(80.0), [Col::Mount, Col::Fs, Col::Size, Col::Used, Col::Free, Col::Pct, Col::Read, Col::Write]);
        assert_eq!(names(40.0), [Col::Mount, Col::Size, Col::Free, Col::Pct]);
        // The mount point and the device share what is left.
        let cols = columns(200.0, 1.0);
        assert_eq!(cols[0].2, cols[1].2);
    }

    #[test]
    fn health_and_temperature_rows_say_how_worried_to_be() {
        let d = |health, why| Drive { health, why, ..Drive::default() };
        assert_eq!(health(&d(Some(Health::Good), None)), (tr("disk.health.good").to_string(), theme::green()));
        assert_eq!(health(&d(Some(Health::Failing), None)).1, theme::red());
        assert_eq!(health(&d(None, Some(crate::collect::disk::Why::NeedsRoot))), (tr("disk.why.needs_root").to_string(), theme::dim()));
        let t = |value, high, crit| Temp { value, high, crit, ..Temp::default() };
        assert_eq!(temp_row(&t(38.0, None, None)), ("38°C".to_string(), theme::fg()));
        assert_eq!(temp_row(&t(38.0, Some(70.0), Some(80.0))), (trf("disk.temp_mark", &["38°C", "70°C"]), theme::fg()));
        assert_eq!(temp_row(&t(72.0, Some(70.0), Some(80.0))).1, theme::yellow());
        assert_eq!(temp_row(&t(85.0, None, Some(80.0))), (trf("disk.temp_mark", &["85°C", "80°C"]), theme::red()));
    }

    #[test]
    fn smart_rows_show_what_the_drive_counts_and_warn_on_wear_and_errors() {
        assert_eq!(smart_rows(&Smart::default()), []);
        assert_eq!((group(0), group(999), group(1000), group(1_234_567)), ("0".into(), "999".into(), "1,000".into(), "1,234,567".into()));
        let s = Smart { life_used: Some(3), power_on_hours: Some(5678), written: Some(1 << 40), media_errors: Some(0), ..Smart::default() };
        let rows = smart_rows(&s);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0], (tr("disk.life_used"), "3%".to_string(), theme::fg()));
        assert_eq!(rows[1], (tr("disk.power_on"), trf("disk.hours", &["5,678", "236"]), theme::fg()));
        assert_eq!(rows[2], (tr("disk.lifetime_writes"), fmt_bytes(1 << 40), theme::fg()));
        assert_eq!(rows[3], (tr("disk.media_errors"), "0".to_string(), theme::fg()));
        let worn = |p| smart_rows(&Smart { life_used: Some(p), ..Smart::default() })[0].2;
        assert_eq!((worn(79), worn(80), worn(100), worn(255)), (theme::fg(), theme::yellow(), theme::red(), theme::red()));
        let s = Smart { remapped: Some(8), pending: Some(1), ..Smart::default() };
        assert_eq!(
            smart_rows(&s),
            [(tr("disk.remapped"), "8".to_string(), theme::yellow()), (tr("disk.pending"), "1".to_string(), theme::yellow())]
        );
        let s = Smart { why: Some(Why::NeedsAdmin), ..Smart::default() };
        assert_eq!(smart_rows(&s), [(tr("disk.smart_counts"), tr("disk.why.needs_admin").to_string(), theme::dim())]);
    }

    #[test]
    fn fuller_volumes_turn_yellow_then_red() {
        assert_eq!(fill_color(10.0), theme::green());
        assert_eq!(fill_color(80.0), theme::yellow());
        assert_eq!(fill_color(95.0), theme::red());
    }
}
