//! Devices (D): what Device Manager lists. The classes on the left (in the
//! warning colour while one of their devices has a problem), the class's
//! devices in the middle (t lays every device out under the one it hangs
//! off, so a hub shows what is plugged into it), the selected device on
//! the right. y copies its id.

use egui::{Color32, Rect, pos2};

use super::{Cx, gpu::para, kv_rows, panel, row_at};
use crate::collect::devices::{Bus, Device, Status, problem_key};
use crate::config::theme::{self, ROW};
use crate::i18n::{tr, trf};
use crate::screen::Pane;

pub fn draw(cx: &mut Cx, [left, mid, right]: [Rect; 3]) {
    draw_left(cx, left);
    draw_mid(cx, mid);
    draw_right(cx, right);
}

pub fn color(s: Status) -> Color32 {
    match s {
        Status::Ok => theme::green(),
        Status::Problem(_) => theme::red(),
        Status::Disabled => theme::yellow(),
        Status::NoDriver => theme::dim(),
    }
}

/// A status as words, with a problem's meaning and code.
pub fn status_text(s: Status) -> String {
    match s {
        Status::Problem(code) => match problem_key(code) {
            Some(k) => trf("dev.problem_code", &[tr(k), &code.to_string()]),
            None => trf("dev.problem_code", &[tr(s.key()), &code.to_string()]),
        },
        _ => tr(s.key()).to_string(),
    }
}

/// A link's speed in kbit/s as a person says it: 1.5 Mbit/s, 480 Mbit/s,
/// 10 Gbit/s.
pub fn speed(kbps: u32) -> String {
    if kbps >= 1_000_000 {
        let g = kbps as f64 / 1_000_000.0;
        if g.fract() == 0.0 { format!("{g:.0} Gbit/s") } else { format!("{g:.1} Gbit/s") }
    } else {
        let m = kbps as f64 / 1_000.0;
        if m.fract() == 0.0 { format!("{m:.0} Mbit/s") } else { format!("{m:.1} Mbit/s") }
    }
}

fn draw_left(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let read = app.model.devices.is_some();
    let total = app.devices().len();
    let info = if read { total.to_string() } else { String::new() };
    let body = panel(&cx.pen, r, tr("dev.classes"), &info, cx.focused(Pane::Left));
    if !read {
        cx.pen.text(pos2(body.left() + 4.0, body.top() + ROW / 2.0), tr("misc.reading"), theme::dim());
        return;
    }
    let filtered = !app.filter().is_empty();
    let n = |shown: usize, all: usize| if filtered { format!("{shown}/{all}") } else { all.to_string() };
    let shown_all: usize = app.dev_classes.iter().map(|c| c.shown).sum();
    let problem = app.dev_classes.iter().any(|c| c.problem);
    cx.scroll_list(Pane::Left, body, 1 + app.dev_classes.len(), |cx, row, i| {
        let (name, count, warn) = match i.checked_sub(1).and_then(|k| app.dev_classes.get(k)) {
            None => (tr("dev.all").to_string(), n(shown_all, total), problem),
            Some(c) => (if c.problem { format!("! {}", c.name) } else { c.name.clone() }, n(c.shown, c.total), c.problem),
        };
        let lc = if warn {
            theme::red()
        } else if i == 0 {
            theme::cyan()
        } else {
            theme::fg()
        };
        cx.pen.pair(row, &name, lc, &count, theme::dim());
    });
}

fn draw_mid(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let title = app.selected_dev_class().map_or(tr("dev.title"), |c| c.name.as_str());
    let info = if app.model.devices.is_some() { trf("dev.count", &[&app.dev_rows.len().to_string()]) } else { String::new() };
    let body = panel(&cx.pen, r, title, &info, cx.focused(Pane::Mid));
    let Some(devs) = &app.model.devices else {
        cx.pen.text(pos2(body.left() + 4.0, body.top() + ROW / 2.0), tr("misc.reading"), theme::dim());
        return;
    };
    let mut top = body.top();
    if let Some(n) = devs.note {
        top = para(&cx.pen, body, top, tr(n.key()), theme::yellow());
    }
    let cw = cx.pen.cw;
    let w = body.width() - 12.0;
    // The vendor, then the status, go while the name would be under 16 characters.
    let status_w = cw * 10.0;
    let show_status = w - status_w - cw >= cw * 16.0;
    let vendor_w = (w * 0.3).clamp(cw * 8.0, cw * 24.0);
    let show_vendor = show_status && w - status_w - vendor_w - cw * 2.0 >= cw * 16.0;
    let name_w = w - if show_status { status_w + cw } else { 0.0 } - if show_vendor { vendor_w + cw } else { 0.0 };
    let x0 = body.left() + 6.0;
    let h = row_at(body, top);
    cx.pen.text(pos2(x0, h.center().y), &cx.pen.fit(tr("dev.col.name"), name_w), theme::dim());
    if show_vendor {
        cx.pen.text(pos2(x0 + name_w + cw, h.center().y), tr("dev.col.vendor"), theme::dim());
    }
    if show_status {
        cx.pen.text(pos2(x0 + w - status_w, h.center().y), tr("dev.col.status"), theme::dim());
    }
    let list = Rect::from_min_max(pos2(body.left(), h.bottom()), body.max);
    if app.dev_rows.is_empty() {
        cx.pen.text(pos2(x0, list.top() + ROW / 2.0), tr("dev.empty"), theme::dim());
        return;
    }
    cx.scroll_list(Pane::Mid, list, app.dev_rows.len(), |cx, row, i| {
        let Some(d) = app.dev_rows.get(i).and_then(|&k| devs.list.get(k)) else { return };
        let cy = row.center().y;
        let mut x = x0;
        if let Some(line) = app.dev_lines.get(i).filter(|l| !l.is_empty()) {
            x = cx.pen.text(pos2(x, cy), line, theme::dim()).right();
        }
        let fg = match d.status {
            Status::Ok => theme::fg(),
            Status::NoDriver => theme::dim(),
            s => color(s),
        };
        cx.pen.text(pos2(x, cy), &cx.pen.fit(&d.name, (x0 + name_w - x).max(cw)), fg);
        if show_vendor {
            cx.pen.text(pos2(x0 + name_w + cw, cy), &cx.pen.fit(&d.vendor, vendor_w), theme::dim());
        }
        if show_status {
            cx.pen.text(pos2(x0 + w - status_w, cy), &cx.pen.fit(tr(d.status.key()), status_w), color(d.status));
        }
    });
}

/// The names of the devices `d` hangs off, nearest last.
fn chain(list: &[Device], d: &Device) -> String {
    let mut names = Vec::new();
    let mut at = d.parent.as_deref();
    // A bound against a loop in what the OS says.
    while let Some(id) = at.filter(|_| names.len() < 16) {
        let Some(p) = list.iter().find(|x| x.id == id) else { break };
        names.push(p.name.as_str());
        at = p.parent.as_deref();
    }
    names.reverse();
    names.join(" › ")
}

fn draw_right(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let Some(d) = app.selected_dev() else {
        let body = panel(&cx.pen, r, tr("dev.detail"), "", cx.focused(Pane::Right));
        if app.model.devices.is_some() {
            cx.pen.text(pos2(body.left() + 4.0, body.top() + ROW / 2.0), tr("dev.none"), theme::dim());
        }
        return;
    };
    let body = panel(&cx.pen, r, &d.name, tr(d.status.key()), cx.focused(Pane::Right));
    let dash = || "—".to_string();
    let or_dash = |s: &str| if s.is_empty() { dash() } else { s.to_string() };
    let (ids_key, rev, link) = match d.bus {
        Bus::Usb { rev, speed_kbps, .. } => ("dev.vid_pid", rev.map(|r| format!("{}.{:02x}", r >> 8, r & 0xff)), speed_kbps.map(speed)),
        Bus::Pci { .. } => ("dev.ven_dev", None, None),
        Bus::Other => ("dev.ids", None, None),
    };
    let mut rows = vec![
        (tr("dev.class"), d.class()),
        (tr("dev.kind_row"), tr(d.kind.key()).to_string()),
        (tr("dev.vendor"), or_dash(&d.vendor)),
        (tr("dev.status_row"), status_text(d.status)),
        (tr("dev.driver"), or_dash(&d.driver)),
        (tr("dev.driver_version"), or_dash(&d.driver_version)),
        (tr(ids_key), d.ids().unwrap_or_else(dash)),
    ];
    if let Some(rev) = rev {
        rows.push((tr("dev.rev"), rev));
    }
    if let Some(link) = link {
        rows.push((tr("dev.speed"), link));
    }
    let parents = chain(app.devices(), d);
    rows.extend([(tr("dev.location"), or_dash(&d.location)), (tr("dev.parent"), or_dash(&parents)), (tr("dev.id"), d.id.clone())]);
    let mut y = kv_rows(&cx.pen, body, body.top(), &rows) + super::GAP;
    if let Status::Problem(_) = d.status {
        y = para(&cx.pen, body, y, tr("dev.problem_hint"), theme::yellow());
    }
    para(&cx.pen, body, y, tr("dev.hint"), theme::dim());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds_read_as_people_say_them() {
        assert_eq!(speed(1_500), "1.5 Mbit/s");
        assert_eq!(speed(12_000), "12 Mbit/s");
        assert_eq!(speed(480_000), "480 Mbit/s");
        assert_eq!(speed(5_000_000), "5 Gbit/s");
        assert_eq!(speed(2_500_000), "2.5 Gbit/s");
    }

    #[test]
    fn the_chain_names_the_hubs_above() {
        let dev = |id: &str, parent: Option<&str>, name: &str| Device {
            id: id.into(),
            parent: parent.map(Into::into),
            name: name.into(),
            ..Device::default()
        };
        let list = [dev("a", None, "Root hub"), dev("b", Some("a"), "Hub"), dev("c", Some("b"), "Mouse"), dev("x", Some("x"), "Loop")];
        assert_eq!(chain(&list, &list[2]), "Root hub › Hub");
        assert_eq!(chain(&list, &list[0]), "");
        assert_eq!(chain(&list, &list[3]).matches("Loop").count(), 16);
    }
}
