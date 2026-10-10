//! Overview (1): the gauges (CPU, memory, swap, each GPU and NPU, each drive)
//! and the system on the left, a graph for each gauge in the middle, the
//! top processes on the right.

use egui::{Color32, Id, Rect, Sense, pos2};

use super::{Cx, GAP, graph, kv_rows, now_secs, panel, row_at, shown};
use crate::app::{App, Gauge, OVW_TOP};
use crate::collect::gpu;
use crate::config::theme::{self, ROW, ramp};
use crate::i18n::tr;
use crate::model::Ring;
use crate::screen::Pane;
use crate::stacks;
use crate::util::{fmt_bytes, fmt_uptime};

/// How short a gauge's graph may get before the middle takes a second
/// column.
const MIN_GRAPH: f32 = 90.0;

/// A row of the left column as it is drawn.
struct Row<'a> {
    label: String,
    /// What the device is (its name or model), for the graph's title and
    /// the system rows; empty for CPU, memory and swap.
    what: String,
    /// `None` when the device does not say.
    pct: Option<f32>,
    color: Color32,
    ring: Option<&'a Ring>,
}

impl Row<'_> {
    fn pct_text(&self) -> String {
        self.pct.map_or_else(|| "—".into(), |p| format!("{p:.0}%"))
    }

    fn pct_color(&self) -> Color32 {
        self.pct.map_or_else(theme::dim, ramp)
    }

    fn title(&self) -> String {
        if self.what.is_empty() { self.label.clone() } else { format!("{} · {}", self.label, self.what) }
    }
}

/// `GPU`, or `GPU 1`, `GPU 2` when there are several of the kind.
fn numbered(base: &str, nth: usize, of: usize) -> String {
    if of > 1 { format!("{base} {}", nth + 1) } else { base.to_string() }
}

/// The left column's rows, in [`App::ovw_gauges`]' order.
fn gauges(app: &App) -> Vec<Row<'_>> {
    let m = &app.model;
    let count = |k: gpu::Kind| m.gpus.iter().filter(|d| d.kind == k).count();
    let (n_gpu, n_npu, n_disk) = (count(gpu::Kind::Gpu), count(gpu::Kind::Npu), m.disks.drives.len());
    app.ovw_gauges()
        .into_iter()
        .map(|g| match g {
            Gauge::Cpu => {
                Row { label: tr("ovw.cpu").into(), what: String::new(), pct: Some(m.total), color: theme::blue(), ring: Some(&m.cpu_hist) }
            }
            Gauge::Mem => Row {
                label: tr("ovw.mem").into(),
                what: String::new(),
                pct: Some(m.mem_pct()),
                color: theme::green(),
                ring: Some(&m.mem_hist),
            },
            Gauge::Swap => {
                let pct = m.swap_hist.last().unwrap_or(0.0);
                Row { label: tr("ovw.swap").into(), what: String::new(), pct: Some(pct), color: theme::magenta(), ring: Some(&m.swap_hist) }
            }
            Gauge::Gpu(i) => {
                let d = &m.gpus[i];
                let nth = m.gpus[..i].iter().filter(|x| x.kind == d.kind).count();
                let (base, of) = match d.kind {
                    gpu::Kind::Gpu => (tr("ovw.gpu"), n_gpu),
                    gpu::Kind::Npu => (tr("ovw.npu"), n_npu),
                };
                let what = if d.name.is_empty() { d.vendor.unwrap_or_default().to_string() } else { d.name.clone() };
                let ring = m.gpu_hist.get(&d.id).map(|h| &h.0);
                Row { label: numbered(base, nth, of), what, pct: d.usage, color: theme::yellow(), ring }
            }
            Gauge::Disk(i) => {
                let d = &m.disks.drives[i];
                let mut what = if d.model.is_empty() { d.name.clone() } else { d.model.clone() };
                if let Some(size) = d.size {
                    what = format!("{what} · {}", fmt_bytes(size));
                }
                let ring = m.drive_hist.get(&d.id).map(|h| &h.2);
                Row { label: numbered(tr("ovw.disk"), i, n_disk), what, pct: d.busy, color: theme::cyan(), ring }
            }
        })
        .collect()
}

pub fn draw(cx: &mut Cx, [left, mid, right]: [Rect; 3]) {
    draw_left(cx, left);
    draw_mid(cx, mid);
    draw_right(cx, right);
}

fn draw_left(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let body = panel(&cx.pen, r, tr("ovw.usage"), "", cx.focused(Pane::Left));
    let mut y = body.top();
    let gs = gauges(app);
    let lw = gs.iter().map(|g| cx.pen.width(&g.label)).fold(0.0, f32::max) + cx.pen.cw;
    for (i, g) in gs.iter().enumerate() {
        if y + ROW > body.bottom() + 0.5 {
            break;
        }
        let row = row_at(body, y);
        cx.row_bg(Pane::Left, i, row);
        let cy = row.center().y;
        cx.pen.text(pos2(row.left() + 6.0, cy), &g.label, theme::fg());
        cx.pen.text_r(pos2(row.right() - 6.0, cy), &g.pct_text(), g.pct_color());
        let bar = Rect::from_min_max(pos2(row.left() + 6.0 + lw, cy - 3.0), pos2(row.right() - 6.0 - cx.pen.cw * 5.0, cy + 3.0));
        if bar.width() > 8.0 {
            graph::bar(&cx.pen, bar, g.pct.unwrap_or(0.0), g.color);
        }
        y += ROW;
    }
    let Some(st) = &app.model.st else { return };
    y += GAP;
    cx.pen.section(row_at(body, y), tr("ovw.system"));
    y += ROW;
    let up = fmt_uptime(now_secs().saturating_sub(st.boot_time));
    let mut rows = vec![
        (tr("sys.host"), st.host.clone()),
        (tr("sys.os"), st.os.clone()),
        (tr("sys.kernel"), st.kernel.clone()),
        (tr("sys.arch"), st.arch.clone()),
        (tr("sys.cpu"), st.brand.clone()),
        (tr("sys.uptime"), up),
    ];
    // What each GPU, NPU and drive is, under the label of its gauge.
    rows.extend(gs.iter().filter(|g| !g.what.is_empty()).map(|g| (g.label.as_str(), g.what.clone())));
    kv_rows(&cx.pen, body, y, &rows);
}

fn draw_mid(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let gs = gauges(app);
    let n = gs.len() as u8;
    let sel = cx.sel(Pane::Left);
    // One graph per gauge, the column shared equally; a second or third
    // column only when a graph would get shorter than `MIN_GRAPH`.
    let rects = cx.stack(Pane::Mid, r, stacks::grid(n, stacks::cols_for(n, r.height(), MIN_GRAPH, 3)));
    for (i, g) in gs.iter().enumerate() {
        let Some(&pr) = rects.get(i).filter(|pr| shown(**pr)) else { continue };
        let body = panel(&cx.pen, pr, &g.title(), &g.pct_text(), i == sel);
        if let Some(ring) = g.ring {
            graph::line(&cx.pen, body, ring.iter(), 100.0, g.color);
        }
        // A click on the graph selects its gauge, as its row on the left.
        if cx.ui.interact(pr, Id::new(("ovw-gauge", i)), Sense::click()).clicked() {
            cx.out.click = Some((Pane::Left, i, false));
        }
    }
}

fn draw_right(cx: &mut Cx, r: Rect) {
    let app = cx.app;
    let body = panel(&cx.pen, r, tr("ovw.top"), "", cx.focused(Pane::Right));
    let n_cpu = app.top_cpu.len().min(OVW_TOP);
    let mut y = body.top();
    cx.pen.section(row_at(body, y), tr("ovw.by_cpu"));
    y += ROW;
    for i in 0..app.count(app.screen, Pane::Right) {
        if i == n_cpu {
            y += 4.0;
            if y + ROW > body.bottom() {
                break;
            }
            cx.pen.section(row_at(body, y), tr("ovw.by_mem"));
            y += ROW;
        }
        if y + ROW > body.bottom() + 0.5 {
            break;
        }
        let Some(p) = app.ovw_top(i).and_then(|k| app.proc_at(k)) else { continue };
        let row = row_at(body, y);
        cx.row_bg(Pane::Right, i, row);
        if i < n_cpu {
            cx.pen.pair(row, &p.name, theme::fg(), &format!("{:.1}%", p.cpu), ramp(p.cpu));
        } else {
            cx.pen.pair(row, &p.name, theme::fg(), &fmt_bytes(p.mem), theme::green());
        }
        y += ROW;
    }
}
