//! The app's state and what each [`Act`] does. Drawing is in `ui/`; this
//! side owns the selection, the layouts and the sampler's channels.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use tsumugi_layout::Node;

use crate::collect::{Ctl, Msg, Proc, gpu, net, svc};
use crate::config::cmd::{Act, Goto, Step};
use crate::config::keymap::{Keymap, Match, resolve};
use crate::config::keys::{Code, Key};
use crate::i18n::{tr, trf};
use crate::kill::{self, Outcome, Target};
use crate::model::Model;
use crate::persist::Saver;
use crate::screen::{Pane, Screen};

/// The sampling intervals `+` and `-` step through, in seconds.
pub const INTERVALS: [f32; 6] = [0.5, 1.0, 2.0, 3.0, 5.0, 10.0];
/// How long a toast stays in the status bar.
pub const TOAST: Duration = Duration::from_secs(3);
/// How far a resize key moves a divider, as a share of the room.
const NUDGE: f32 = 0.03;
/// Rows the overview lists in each of its two top lists.
pub const OVW_TOP: usize = 5;
const TOP_CPU: usize = 10;
const TOP_MEM: usize = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Filter,
    Help,
    /// The question before a process is ended.
    Confirm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortCol {
    Pid,
    Name,
    Cpu,
    Mem,
    Thr,
    User,
}

impl SortCol {
    pub const ALL: [SortCol; 6] = [Self::Pid, Self::Name, Self::Cpu, Self::Mem, Self::Thr, Self::User];

    pub fn label(self) -> &'static str {
        match self {
            Self::Pid => tr("procs.col.pid"),
            Self::Name => tr("procs.col.name"),
            Self::Cpu => tr("procs.col.cpu"),
            Self::Mem => tr("procs.col.mem"),
            Self::Thr => tr("procs.col.thr"),
            Self::User => tr("procs.col.user"),
        }
    }

    fn next(self) -> Self {
        let i = Self::ALL.iter().position(|&c| c == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

/// Where each screen's cursor is: the focused column and a row per column.
#[derive(Clone, Copy, Debug, Default)]
pub struct View {
    pub focus: usize,
    pub sel: [usize; 3],
    pub scroll: [usize; 3],
}

/// What a frame's drawing reports back, applied once drawing is done.
#[derive(Default)]
pub struct Out {
    /// A row clicked: its column, its index, and whether it was the second
    /// click of a double click.
    pub click: Option<(Pane, usize, bool)>,
    pub scroll: [Option<usize>; 3],
    pub visible: [usize; 3],
    pub pane_rects: [Option<egui::Rect>; 3],
    pub tab: Option<Screen>,
    /// A divider dragged, released or double-clicked.
    pub moved: Option<tsumugi_layout::ui::Moved<Pane>>,
    /// How far the help can scroll, so a key past the end stays there.
    pub help_max: Option<usize>,
}

pub struct App {
    km: Keymap,
    pub model: Model,
    rx: Receiver<Msg>,
    ctl: Sender<Ctl>,
    pub screen: Screen,
    pub layouts: HashMap<Screen, Node<Pane>>,
    /// The layout while a divider is being dragged; kept on release.
    pub drag: Option<Node<Pane>>,
    views: HashMap<Screen, View>,
    pub visible: [usize; 3],
    pub mode: Mode,
    pending: Vec<Key>,
    pub filter: String,
    pub sort: SortCol,
    proc_pid: Option<u32>,
    /// The process list as procs shows it: indices into `model.procs`.
    pub rows: Vec<usize>,
    /// procs as a tree (`t`) rather than a flat list.
    pub tree: bool,
    /// The branch lines in front of each row's name, while `tree` is on.
    pub tree_lines: Vec<String>,
    pub top_cpu: Vec<usize>,
    pub top_mem: Vec<usize>,
    /// The service list as svc shows it: indices into `model.services.list`.
    pub svc_rows: Vec<usize>,
    /// The service selected, kept while the list is read again.
    svc_name: Option<String>,
    /// The socket list as net shows it: indices into `model.conns.list`.
    pub net_rows: Vec<usize>,
    /// The socket selected, kept while the list is read again.
    net_key: Option<ConnKey>,
    /// net lists only the sockets waiting for others (`u`).
    pub net_listen: bool,
    me: Option<String>,
    dirty: bool,
    pub interval: f32,
    pub paused: bool,
    pub toast: Option<(String, Instant)>,
    pub help_scroll: usize,
    /// The process the confirm question is about.
    pub confirm: Option<Target>,
    kill_tx: Sender<(Target, Outcome)>,
    kill_rx: Receiver<(Target, Outcome)>,
    /// Repainted when a process has been ended, so the toast shows at once.
    pub wake: Option<egui::Context>,
    saver: Saver,
    pub quit: bool,
    wheel_acc: f32,
}

impl App {
    pub fn new(km: Keymap, rx: Receiver<Msg>, ctl: Sender<Ctl>, layouts: HashMap<Screen, Node<Pane>>, saver: Saver, interval: f32) -> Self {
        let (kill_tx, kill_rx) = crossbeam_channel::unbounded();
        Self {
            km,
            model: Model::default(),
            rx,
            ctl,
            screen: Screen::Ovw,
            layouts,
            drag: None,
            views: HashMap::new(),
            visible: [1; 3],
            mode: Mode::Normal,
            pending: Vec::new(),
            filter: String::new(),
            sort: SortCol::Cpu,
            proc_pid: None,
            rows: Vec::new(),
            tree: false,
            tree_lines: Vec::new(),
            top_cpu: Vec::new(),
            top_mem: Vec::new(),
            svc_rows: Vec::new(),
            svc_name: None,
            net_rows: Vec::new(),
            net_key: None,
            net_listen: false,
            me: None,
            dirty: true,
            interval,
            paused: false,
            toast: None,
            help_scroll: 0,
            confirm: None,
            kill_tx,
            kill_rx,
            wake: None,
            saver,
            quit: false,
            wheel_acc: 0.0,
        }
    }

    pub fn keymap(&self) -> &Keymap {
        &self.km
    }

    pub fn view(&self) -> View {
        self.views.get(&self.screen).copied().unwrap_or_default()
    }

    pub fn view_of(&self, s: Screen) -> View {
        self.views.get(&s).copied().unwrap_or_default()
    }

    fn view_mut(&mut self) -> &mut View {
        self.views.entry(self.screen).or_default()
    }

    pub fn focus(&self) -> Pane {
        Pane::ALL[self.view().focus.min(2)]
    }

    pub fn layout(&self) -> &Node<Pane> {
        self.drag.as_ref().unwrap_or(&self.layouts[&self.screen])
    }

    pub fn toast(&mut self, s: impl Into<String>) {
        self.toast = Some((s.into(), Instant::now()));
    }

    /// Fold in what the sampler sent and bring the lists up to date.
    pub fn pump(&mut self) {
        let (mut svc_dirty, mut net_dirty) = (false, false);
        while let Ok(m) = self.rx.try_recv() {
            match m {
                // The net filter matches process names too.
                Msg::Procs { .. } => (self.dirty, net_dirty) = (true, true),
                Msg::Svc(_) => svc_dirty = true,
                Msg::Conns(_) => net_dirty = true,
                _ => {}
            }
            self.model.apply(m);
        }
        if svc_dirty {
            self.refresh_svc();
        }
        if net_dirty && self.model.conn_read {
            self.refresh_net();
        }
        while let Ok((t, o)) = self.kill_rx.try_recv() {
            let (name, pid) = (t.name.as_str(), t.pid.to_string());
            let s = match o {
                Outcome::Ended => trf("toast.ended", &[name, &pid]),
                Outcome::Killed => trf("toast.killed", &[name, &pid]),
                Outcome::Gone => trf("toast.gone", &[name, &pid]),
                Outcome::Failed(e) => trf("toast.kill_failed", &[name, &pid, &e]),
            };
            self.toast(s);
        }
        if self.dirty {
            self.dirty = false;
            self.refresh_rows();
        }
        self.clamp_all();
        // Dropped once it has had its time, or the frame after it would ask
        // for another frame forever.
        if self.toast.as_ref().is_some_and(|(_, at)| at.elapsed() >= TOAST) {
            self.toast = None;
        }
    }

    /// How long the toast has left in the status bar.
    pub fn toast_left(&self) -> Option<Duration> {
        self.toast.as_ref().map(|(_, at)| TOAST.saturating_sub(at.elapsed()))
    }

    pub fn proc_at(&self, i: usize) -> Option<&Proc> {
        self.model.procs.get(i)
    }

    /// The process the procs screen has selected.
    pub fn selected_proc(&self) -> Option<&Proc> {
        let sel = self.view_of(Screen::Procs).sel[1];
        self.rows.get(sel).and_then(|&i| self.proc_at(i))
    }

    /// The process under the cursor: procs' selection, or the row of a
    /// top list in the focused column.
    pub fn focused_proc(&self) -> Option<&Proc> {
        let sel = self.view().sel[self.focus().index()];
        let i = match (self.screen, self.focus()) {
            (Screen::Procs, _) => return self.selected_proc(),
            (Screen::Ovw, Pane::Right) => self.ovw_top(sel),
            (Screen::Cpu, Pane::Right) => self.top_cpu.get(sel).copied(),
            (Screen::Mem, Pane::Right) => self.top_mem.get(sel).copied(),
            (Screen::Gpu, Pane::Right) => {
                let pid = self.gpu_procs().get(sel).map(|g| g.pid);
                pid.and_then(|pid| self.model.procs.iter().position(|p| p.pid == pid))
            }
            _ => None,
        };
        i.and_then(|i| self.proc_at(i))
    }

    /// The device the gpu screen has selected.
    pub fn selected_gpu(&self) -> Option<&gpu::Device> {
        self.model.gpus.get(self.view_of(Screen::Gpu).sel[0])
    }

    /// The processes on the selected device.
    pub fn gpu_procs(&self) -> &[gpu::GpuProc] {
        self.selected_gpu().map_or(&[], |d| &d.procs)
    }

    /// The service the svc screen has selected.
    pub fn selected_svc(&self) -> Option<&svc::Service> {
        let sel = self.view_of(Screen::Svc).sel[1];
        self.svc_rows.get(sel).and_then(|&i| self.model.services.list.get(i))
    }

    /// The states svc's left column filters by, in its order.
    pub const SVC_SHOW: [Option<svc::State>; 4] = [None, Some(svc::State::Running), Some(svc::State::Stopped), Some(svc::State::Failed)];

    fn svc_matches(s: &svc::Service, state: Option<svc::State>, f: &str) -> bool {
        state.is_none_or(|st| s.state == st) && (f.is_empty() || s.name.to_lowercase().contains(f) || s.display.to_lowercase().contains(f))
    }

    /// How many services a row of svc's left column would show.
    pub fn count_svc(&self, state: Option<svc::State>) -> usize {
        let f = self.filter.to_lowercase();
        self.model.services.list.iter().filter(|s| Self::svc_matches(s, state, &f)).count()
    }

    fn refresh_svc(&mut self) {
        let f = self.filter.to_lowercase();
        let state = Self::SVC_SHOW[self.view_of(Screen::Svc).sel[0].min(3)];
        let list = &self.model.services.list;
        self.svc_rows = (0..list.len()).filter(|&i| Self::svc_matches(&list[i], state, &f)).collect();
        let found = self.svc_name.as_ref().and_then(|n| self.svc_rows.iter().position(|&i| list[i].name == *n));
        if let Some(pos) = found {
            self.views.entry(Screen::Svc).or_default().sel[1] = pos;
        }
        self.clamp_all();
        self.svc_name = self.selected_svc().map(|s| s.name.clone());
    }

    /// The interface the net screen has selected.
    pub fn selected_nic(&self) -> Option<&net::Nic> {
        self.model.nics.get(self.view_of(Screen::Net).sel[0])
    }

    /// The socket the net screen has selected.
    pub fn selected_conn(&self) -> Option<&net::Conn> {
        let sel = self.view_of(Screen::Net).sel[1];
        self.net_rows.get(sel).and_then(|&i| self.model.conns.list.get(i))
    }

    /// The name of the process with `pid`, if it is in the process list.
    pub fn proc_name(&self, pid: Option<u32>) -> Option<&str> {
        let pid = pid?;
        self.model.procs.iter().find(|p| p.pid == pid).map(|p| p.name.as_str())
    }

    fn conn_matches(&self, c: &net::Conn, f: &str) -> bool {
        if self.net_listen && !c.listening() {
            return false;
        }
        if f.is_empty() {
            return true;
        }
        let local = c.local.to_string();
        let remote = c.remote.map(|r| r.to_string()).unwrap_or_default();
        let pid = c.pid.map(|p| p.to_string()).unwrap_or_default();
        let name = self.proc_name(c.pid).unwrap_or_default().to_lowercase();
        let state = tr(c.state.key()).to_lowercase();
        [c.proto.label(), &local, &remote, &pid, &name, &state].iter().any(|s| s.contains(f))
    }

    fn refresh_net(&mut self) {
        let f = self.filter.to_lowercase();
        let list = &self.model.conns.list;
        let rows: Vec<usize> = (0..list.len()).filter(|&i| self.conn_matches(&list[i], &f)).collect();
        let found = self.net_key.and_then(|k| rows.iter().position(|&i| conn_key(&list[i]) == k));
        self.net_rows = rows;
        if let Some(pos) = found {
            self.views.entry(Screen::Net).or_default().sel[1] = pos;
        }
        self.clamp_all();
        self.net_key = self.selected_conn().map(conn_key);
    }

    /// Every list the filter narrows.
    fn refilter(&mut self) {
        self.refresh_rows();
        self.refresh_svc();
        self.refresh_net();
    }

    pub fn mine_only(&self) -> bool {
        self.view_of(Screen::Procs).sel[0] == 1
    }

    pub fn my_user(&self) -> Option<&str> {
        self.me.as_deref()
    }

    fn matches(&self, p: &Proc, mine: bool, f: &str) -> bool {
        if mine && (self.me.is_none() || p.user != self.me) {
            return false;
        }
        f.is_empty() || p.name.to_lowercase().contains(f) || p.pid.to_string().contains(f) || p.cmd.to_lowercase().contains(f)
    }

    /// The count of processes the procs list would show for a scope.
    pub fn count_scope(&self, mine: bool) -> usize {
        let f = self.filter.to_lowercase();
        self.model.procs.iter().filter(|p| self.matches(p, mine, &f)).count()
    }

    fn refresh_rows(&mut self) {
        if self.me.is_none() {
            let me = std::process::id();
            self.me = self.model.procs.iter().find(|p| p.pid == me).and_then(|p| p.user.clone());
        }
        let f = self.filter.to_lowercase();
        let mine = self.mine_only();
        let procs = &self.model.procs;
        let mut rows: Vec<usize> = (0..procs.len()).filter(|&i| self.matches(&procs[i], mine, &f)).collect();
        let key = |a: &Proc, b: &Proc| -> std::cmp::Ordering {
            let o = match self.sort {
                SortCol::Pid => a.pid.cmp(&b.pid),
                SortCol::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortCol::Cpu => b.cpu.total_cmp(&a.cpu),
                SortCol::Mem => b.mem.cmp(&a.mem),
                SortCol::Thr => b.threads.cmp(&a.threads),
                SortCol::User => a.user.cmp(&b.user),
            };
            o.then(a.pid.cmp(&b.pid))
        };
        rows.sort_by(|&a, &b| key(&procs[a], &procs[b]));
        // The tree keeps the sort among siblings.
        let lines = if self.tree {
            let t = crate::tree::order(procs, &rows);
            rows = t.iter().map(|r| r.0).collect();
            t.into_iter().map(|r| r.1).collect()
        } else {
            Vec::new()
        };
        let mut by_cpu: Vec<usize> = (0..procs.len()).collect();
        by_cpu.sort_by(|&a, &b| procs[b].cpu.total_cmp(&procs[a].cpu).then(procs[a].pid.cmp(&procs[b].pid)));
        by_cpu.truncate(TOP_CPU);
        let mut by_mem: Vec<usize> = (0..procs.len()).collect();
        by_mem.sort_by(|&a, &b| procs[b].mem.cmp(&procs[a].mem).then(procs[a].pid.cmp(&procs[b].pid)));
        by_mem.truncate(TOP_MEM);
        self.rows = rows;
        self.tree_lines = lines;
        self.top_cpu = by_cpu;
        self.top_mem = by_mem;

        // Keep the same process selected while the list reorders.
        let found = self.proc_pid.and_then(|pid| self.rows.iter().position(|&i| self.model.procs[i].pid == pid));
        let v = self.views.entry(Screen::Procs).or_default();
        if let Some(pos) = found {
            v.sel[1] = pos;
        }
        self.clamp_all();
        self.proc_pid = self.selected_proc().map(|p| p.pid);
    }

    /// Rows a column has to select, on a screen.
    pub fn count(&self, s: Screen, p: Pane) -> usize {
        let groups = self.model.st.as_ref().map_or(0, |st| st.topo.groups.len());
        let cores = self.model.st.as_ref().map_or(0, |st| st.topo.core_count());
        match (s, p) {
            (Screen::Ovw, Pane::Left) => 3,
            (Screen::Ovw, Pane::Right) => self.top_cpu.len().min(OVW_TOP) + self.top_mem.len().min(OVW_TOP),
            (Screen::Cpu, Pane::Left) => 1 + groups,
            (Screen::Cpu, Pane::Mid) => cores,
            (Screen::Cpu, Pane::Right) => self.top_cpu.len(),
            (Screen::Mem, Pane::Left) => 3,
            (Screen::Mem, Pane::Right) => self.top_mem.len(),
            (Screen::Gpu, Pane::Left) => self.model.gpus.len(),
            (Screen::Gpu, Pane::Right) => self.gpu_procs().len(),
            (Screen::Procs, Pane::Left) => 2,
            (Screen::Procs, Pane::Mid) => self.rows.len(),
            (Screen::Svc, Pane::Left) => Self::SVC_SHOW.len(),
            (Screen::Svc, Pane::Mid) => self.svc_rows.len(),
            (Screen::Net, Pane::Left) => self.model.nics.len(),
            (Screen::Net, Pane::Mid) => self.net_rows.len(),
            _ => 0,
        }
    }

    /// The overview's right column: the top CPU users, then the top memory.
    pub fn ovw_top(&self, i: usize) -> Option<usize> {
        let n = self.top_cpu.len().min(OVW_TOP);
        if i < n { self.top_cpu.get(i).copied() } else { self.top_mem.get(i - n).copied() }
    }

    fn clamp_all(&mut self) {
        let counts: Vec<(Screen, [usize; 3])> = Screen::ALL.iter().map(|&s| (s, Pane::ALL.map(|p| self.count(s, p)))).collect();
        for (s, c) in counts {
            let v = self.views.entry(s).or_default();
            for (sel, n) in v.sel.iter_mut().zip(c) {
                *sel = (*sel).min(n.saturating_sub(1));
            }
        }
    }

    /// Apply what drawing reported.
    pub fn apply_out(&mut self, mut out: Out, ctx: &egui::Context) {
        for p in 0..3 {
            if let Some(s) = out.scroll[p] {
                self.view_mut().scroll[p] = s;
            }
        }
        self.visible = out.visible.map(|v| v.max(1));
        if let Some(m) = out.help_max {
            self.help_scroll = self.help_scroll.min(m);
        }
        if let Some(s) = out.tab {
            self.goto(s);
        }
        if let Some(m) = out.moved.take() {
            self.moved(m);
        }
        if let Some((pane, i, double)) = out.click {
            let v = self.view_mut();
            v.focus = pane.index();
            v.sel[pane.index()] = i;
            self.selected_moved(pane);
            if double {
                self.run(Act::Enter);
            }
        }
        self.wheel(ctx, &out);
    }

    fn wheel(&mut self, ctx: &egui::Context, out: &Out) {
        let (delta, pos) = ctx.input(|i| {
            let mut d = 0.0;
            for e in &i.events {
                if let egui::Event::MouseWheel { unit, delta, .. } = e {
                    d += match unit {
                        egui::MouseWheelUnit::Line => delta.y * 3.0,
                        egui::MouseWheelUnit::Point => delta.y / crate::config::theme::ROW,
                        egui::MouseWheelUnit::Page => delta.y * 10.0,
                    };
                }
            }
            (d, i.pointer.hover_pos())
        });
        if delta == 0.0 || self.mode != Mode::Normal {
            return;
        }
        let Some(pos) = pos else { return };
        let Some(p) = (0..3).find(|&p| out.pane_rects[p].is_some_and(|r| r.contains(pos))) else { return };
        self.wheel_acc += delta;
        let rows = self.wheel_acc.trunc();
        if rows != 0.0 {
            self.wheel_acc -= rows;
            self.move_sel(Pane::ALL[p], -(rows as i64));
        }
    }

    fn move_sel(&mut self, pane: Pane, by: i64) {
        let n = self.count(self.screen, pane);
        if n == 0 {
            return;
        }
        let v = self.view_mut();
        let cur = v.sel[pane.index()] as i64;
        v.sel[pane.index()] = (cur + by).clamp(0, n as i64 - 1) as usize;
        self.selected_moved(pane);
    }

    /// Follow-ups of a new selection: procs remembers the process, the
    /// procs scope re-filters.
    fn selected_moved(&mut self, pane: Pane) {
        match (self.screen, pane) {
            (Screen::Procs, Pane::Mid) => self.proc_pid = self.selected_proc().map(|p| p.pid),
            (Screen::Procs, Pane::Left) => self.refresh_rows(),
            (Screen::Svc, Pane::Mid) => self.svc_name = self.selected_svc().map(|s| s.name.clone()),
            (Screen::Svc, Pane::Left) => self.refresh_svc(),
            (Screen::Net, Pane::Mid) => self.net_key = self.selected_conn().map(conn_key),
            _ => {}
        }
    }

    pub fn goto(&mut self, s: Screen) {
        // Services are read only while their screen shows.
        if (s == Screen::Svc) != (self.screen == Screen::Svc) {
            let _ = self.ctl.send(Ctl::Services(s == Screen::Svc));
        }
        // And sockets while theirs does.
        if (s == Screen::Net) != (self.screen == Screen::Net) {
            let _ = self.ctl.send(Ctl::Net(s == Screen::Net));
        }
        self.screen = s;
        self.drag = None;
        self.pending.clear();
    }

    /// One key, through the layer of the current mode.
    pub fn feed(&mut self, k: Key) {
        match self.mode {
            Mode::Help => {
                if let Match::Exact(b) = resolve(&self.km.help, &[k]) {
                    for a in b.run.clone() {
                        self.run_help(a);
                    }
                }
            }
            Mode::Confirm => {
                if let Match::Exact(b) = resolve(&self.km.confirm, &[k]) {
                    for a in b.run.clone() {
                        self.run_confirm(a);
                    }
                }
            }
            Mode::Filter => match resolve(&self.km.input, &[k]) {
                Match::Exact(b) => {
                    for a in b.run.clone() {
                        self.run(a);
                    }
                }
                _ => {
                    if let (Code::Char(c), false) = (k.code, k.ctrl || k.alt || k.sup) {
                        self.filter.push(c);
                        self.refilter();
                    }
                }
            },
            Mode::Normal => {
                self.pending.push(k);
                match resolve(&self.km.main, &self.pending) {
                    Match::Exact(b) => {
                        let run = b.run.clone();
                        self.pending.clear();
                        for a in run {
                            self.run(a);
                        }
                    }
                    Match::Prefix => {}
                    Match::None => {
                        let again = self.pending.len() > 1;
                        self.pending.clear();
                        if again {
                            self.feed(k);
                        }
                    }
                }
            }
        }
    }

    fn run_help(&mut self, a: Act) {
        match a {
            Act::Escape | Act::Help | Act::Quit => self.mode = Mode::Normal,
            Act::Arrow(Step::By(n)) => self.help_scroll = (self.help_scroll as i64 + n).max(0) as usize,
            Act::Arrow(Step::Top) => self.help_scroll = 0,
            _ => {}
        }
    }

    fn run_confirm(&mut self, a: Act) {
        match a {
            Act::Confirm | Act::Enter => {
                self.mode = Mode::Normal;
                if let Some(t) = self.confirm.take() {
                    self.toast(trf("toast.ending", &[&t.name, &t.pid.to_string()]));
                    kill::spawn(t, self.kill_tx.clone(), self.wake.clone());
                }
            }
            Act::Escape | Act::Quit => {
                self.mode = Mode::Normal;
                self.confirm = None;
            }
            _ => {}
        }
    }

    /// Ask before ending the process under the cursor.
    fn ask_kill(&mut self) {
        let Some(p) = self.focused_proc() else {
            self.toast(tr("toast.kill_none"));
            return;
        };
        if p.pid == std::process::id() {
            self.toast(tr("toast.kill_self"));
            return;
        }
        self.confirm = Some(Target { pid: p.pid, name: p.name.clone(), start: p.start });
        self.mode = Mode::Confirm;
    }

    pub fn run(&mut self, a: Act) {
        let built = self.screen.built();
        match a {
            Act::Screen(g) => {
                let s = match g {
                    Goto::To(s) => s,
                    Goto::Next | Goto::Prev => {
                        let step = if g == Goto::Next { 1 } else { Screen::ALL.len() - 1 };
                        let mut i = self.screen.index();
                        loop {
                            i = (i + step) % Screen::ALL.len();
                            if Screen::ALL[i].built() {
                                break Screen::ALL[i];
                            }
                        }
                    }
                };
                self.goto(s);
            }
            Act::Arrow(step) if built => {
                let pane = self.focus();
                let n = self.count(self.screen, pane) as i64;
                let cur = self.view().sel[pane.index()] as i64;
                let to = match step {
                    Step::By(d) => cur + d,
                    Step::Top => 0,
                    Step::Bot => n - 1,
                    Step::Page(f) => cur + (self.visible[pane.index()] as f32 * f).round() as i64,
                };
                self.move_sel(pane, to - cur);
            }
            Act::Focus(d) if built => {
                let v = self.view_mut();
                v.focus = (v.focus as i32 + d).clamp(0, 2) as usize;
            }
            Act::Resize(t) if built => {
                let focus = self.focus();
                let l = self.layouts.get_mut(&self.screen).expect("every screen has a layout");
                if l.nudge(&focus, t, NUDGE) {
                    self.saver.save(&self.layouts);
                } else {
                    self.toast(tr("toast.no_divider"));
                }
            }
            Act::Equalize if built => {
                if let Some(l) = self.layouts.get_mut(&self.screen) {
                    l.equalize();
                }
                self.saver.save(&self.layouts);
            }
            Act::Enter if built => self.enter(),
            Act::Filter => {
                // svc and net filter their own lists; anywhere else it is procs'.
                if !matches!(self.screen, Screen::Svc | Screen::Net) {
                    self.goto(Screen::Procs);
                }
                self.view_mut().focus = Pane::Mid.index();
                self.mode = Mode::Filter;
            }
            Act::Kill => self.ask_kill(),
            Act::Sort => {
                self.sort = self.sort.next();
                self.refresh_rows();
                self.toast(trf("toast.sort", &[self.sort.label()]));
            }
            Act::Tree => {
                self.tree = !self.tree;
                self.refresh_rows();
                self.toast(tr(if self.tree { "toast.tree_on" } else { "toast.tree_off" }));
            }
            Act::Scope(to) if self.screen == Screen::Net => {
                self.net_listen = to.unwrap_or(!self.net_listen);
                self.refresh_net();
                self.toast(trf("toast.scope", &[tr(if self.net_listen { "net.listening" } else { "net.all_conns" })]));
            }
            Act::Scope(to) => {
                let mine = to.unwrap_or(!self.mine_only());
                self.views.entry(Screen::Procs).or_default().sel[0] = mine as usize;
                self.refresh_rows();
                self.toast(trf("toast.scope", &[tr(if mine { "procs.mine" } else { "procs.all" })]));
            }
            Act::Pause => {
                self.paused = !self.paused;
                let _ = self.ctl.send(Ctl::Pause(self.paused));
                self.toast(tr(if self.paused { "toast.paused" } else { "toast.resumed" }));
            }
            Act::Interval(d) => {
                let cur = INTERVALS.iter().position(|&i| i >= self.interval).unwrap_or(INTERVALS.len() - 1) as i32;
                let i = (cur + d).clamp(0, INTERVALS.len() as i32 - 1) as usize;
                self.interval = INTERVALS[i];
                let _ = self.ctl.send(Ctl::Interval(Duration::from_secs_f32(self.interval)));
                self.toast(trf("toast.interval", &[&fmt_interval(self.interval)]));
            }
            Act::Help => {
                self.mode = Mode::Help;
                self.help_scroll = 0;
            }
            Act::Quit => self.quit = true,
            Act::Escape => {
                if self.mode == Mode::Filter || !self.filter.is_empty() {
                    self.filter.clear();
                    self.refilter();
                }
                self.mode = Mode::Normal;
            }
            Act::Confirm => self.mode = Mode::Normal,
            Act::Backspace if self.filter.pop().is_some() => self.refilter(),
            // Arrow, Focus, Resize, Equalize and Enter on a screen not built yet.
            _ => {}
        }
    }

    fn enter(&mut self) {
        let v = self.view();
        let sel = v.sel[v.focus.min(2)];
        match (self.screen, self.focus()) {
            (Screen::Ovw, Pane::Left) => self.goto(if sel == 0 { Screen::Cpu } else { Screen::Mem }),
            (Screen::Ovw, Pane::Right) => {
                if let Some(i) = self.ovw_top(sel) {
                    self.open_proc(self.model.procs[i].pid);
                }
            }
            (Screen::Cpu, Pane::Left) => {
                let first = match (sel, &self.model.st) {
                    (0, _) | (_, None) => 0,
                    (g, Some(st)) => st.topo.groups.iter().take(g - 1).map(|g| g.cores.len()).sum(),
                };
                let v = self.view_mut();
                v.focus = Pane::Mid.index();
                v.sel[Pane::Mid.index()] = first;
            }
            (Screen::Cpu, Pane::Right) => {
                if let Some(&i) = self.top_cpu.get(sel) {
                    self.open_proc(self.model.procs[i].pid);
                }
            }
            (Screen::Mem, Pane::Right) => {
                if let Some(&i) = self.top_mem.get(sel) {
                    self.open_proc(self.model.procs[i].pid);
                }
            }
            (Screen::Gpu, Pane::Left) if !self.gpu_procs().is_empty() => self.view_mut().focus = Pane::Right.index(),
            (Screen::Gpu, Pane::Right) => {
                if let Some(pid) = self.gpu_procs().get(sel).map(|g| g.pid) {
                    self.open_proc(pid);
                }
            }
            (Screen::Procs, Pane::Left) => self.view_mut().focus = Pane::Mid.index(),
            (Screen::Svc, Pane::Left) => self.view_mut().focus = Pane::Mid.index(),
            (Screen::Svc, Pane::Mid | Pane::Right) => match self.selected_svc().map(|s| (s.pid, s.name.clone())) {
                Some((Some(pid), _)) => self.open_proc(pid),
                Some((None, name)) => self.toast(trf("toast.svc_no_pid", &[&name])),
                None => {}
            },
            (Screen::Net, Pane::Left) => self.view_mut().focus = Pane::Mid.index(),
            (Screen::Net, Pane::Mid | Pane::Right) => match self.selected_conn().map(|c| (c.pid, c.local)) {
                Some((Some(pid), _)) => self.open_proc(pid),
                Some((None, local)) => self.toast(trf("toast.net_no_pid", &[&local.to_string()])),
                None => {}
            },
            _ => {}
        }
    }

    /// Procs with `pid` selected, the scope and filter widened if they hid it.
    fn open_proc(&mut self, pid: u32) {
        self.goto(Screen::Procs);
        let v = self.view_mut();
        v.focus = Pane::Mid.index();
        v.sel[Pane::Left.index()] = 0;
        let f = self.filter.to_lowercase();
        if self.model.procs.iter().any(|p| p.pid == pid && !self.matches(p, false, &f)) {
            self.filter.clear();
        }
        self.proc_pid = Some(pid);
        self.refresh_rows();
    }

    /// A divider dragged or double-clicked with the mouse.
    pub fn moved(&mut self, m: tsumugi_layout::ui::Moved<Pane>) {
        use tsumugi_layout::ui::Moved;
        match m {
            Moved::Dragging(l) => self.drag = Some(l),
            Moved::Released => {
                if let Some(l) = self.drag.take() {
                    self.layouts.insert(self.screen, l);
                    self.saver.save(&self.layouts);
                }
            }
            Moved::Halved(l) => {
                self.drag = None;
                self.layouts.insert(self.screen, l);
                self.saver.save(&self.layouts);
            }
        }
    }

    /// The keys of the current state, for a scripted run's report.
    pub fn report(&self) -> String {
        let v = self.view();
        let mut s = String::new();
        s += &format!("screen: {}\n", self.screen.name());
        s += &format!("mode: {:?}\n", self.mode);
        s += &format!("focus: {}\n", self.focus().name());
        s += &format!("sel: {:?}\n", v.sel);
        s += &format!("filter: {}\n", self.filter);
        s += &format!("sort: {:?}\n", self.sort);
        s += &format!("tree: {}\n", self.tree);
        s += &format!("scope: {}\n", if self.mine_only() { "mine" } else { "all" });
        s += &format!("interval: {}\n", self.interval);
        s += &format!("paused: {}\n", self.paused);
        s += &format!("samples: {}\n", self.model.samples);
        s += &format!("procs: {}\n", self.rows.len());
        let gpus: Vec<&str> = self.model.gpus.iter().map(|d| d.id.as_str()).collect();
        s += &format!("gpus: {} ({})\n", gpus.len(), gpus.join(" "));
        if self.model.svc_read {
            let sv = &self.model.services;
            s += &format!("svcs: {} of {} ({})\n", self.svc_rows.len(), sv.list.len(), sv.source);
        }
        if self.model.nic_read {
            let names: Vec<&str> = self.model.nics.iter().map(|n| n.name.as_str()).collect();
            s += &format!("nics: {} ({})\n", names.len(), names.join(" "));
        }
        if self.model.conn_read {
            let c = &self.model.conns;
            let scope = if self.net_listen { "listening" } else { "all" };
            s += &format!("conns: {} of {} ({}, {scope})\n", self.net_rows.len(), c.list.len(), c.source);
        }
        if let Some(st) = &self.model.st {
            s += &format!("topology: {} ({})\n", st.topo.summary(), st.topo.source);
        }
        let widths: Vec<String> = self.layouts[&self.screen]
            .layout(tsumugi_layout::Rect::new(0.0, 0.0, 1000.0, 100.0), 0.0)
            .iter()
            .map(|(p, r)| format!("{}={:.0}", p.name(), r.w))
            .collect();
        s += &format!("widths: {}\n", widths.join(" "));
        if let Some(t) = &self.confirm {
            s += &format!("confirm: {} {}\n", t.pid, t.name);
        }
        if let Some((t, _)) = &self.toast {
            s += &format!("toast: {t}\n");
        }
        s
    }
}

/// What tells one socket from another while the list is read again.
type ConnKey = (net::Proto, std::net::SocketAddr, Option<std::net::SocketAddr>);

fn conn_key(c: &net::Conn) -> ConnKey {
    (c.proto, c.local, c.remote)
}

pub fn fmt_interval(s: f32) -> String {
    if s.fract() == 0.0 { format!("{s:.0}") } else { format!("{s}") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect::Mem;

    fn proc(pid: u32, name: &str, cpu: f32, mem: u64) -> Proc {
        Proc {
            pid,
            ppid: None,
            name: name.into(),
            cpu,
            mem,
            threads: Some(1),
            user: Some("me".into()),
            cmd: String::new(),
            exe: None,
            start: 0,
            status: "Run".into(),
        }
    }

    fn app() -> (App, Sender<Msg>, Receiver<Ctl>) {
        let (km, _) = Keymap::load(None);
        let (tx, rx) = crossbeam_channel::unbounded();
        let (ctl_tx, ctl_rx) = crossbeam_channel::unbounded();
        let layouts = Screen::ALL.iter().map(|&s| (s, s.default_layout())).collect();
        (App::new(km, rx, ctl_tx, layouts, Saver::new(None), 1.0), tx, ctl_rx)
    }

    fn keys(a: &mut App, s: &str) {
        for k in s.split_whitespace() {
            a.feed(Key::parse(k).unwrap());
        }
    }

    #[test]
    fn the_selection_follows_the_process_while_the_list_reorders() {
        let (mut a, tx, _ctl) = app();
        tx.send(Msg::Mem(Mem::default())).unwrap();
        tx.send(Msg::Procs { procs: vec![proc(1, "a", 50.0, 1), proc(2, "b", 10.0, 2), proc(3, "c", 5.0, 3)], total_threads: None })
            .unwrap();
        a.pump();
        keys(&mut a, "5 l j");
        assert_eq!(a.selected_proc().map(|p| p.pid), Some(2));
        tx.send(Msg::Procs { procs: vec![proc(1, "a", 1.0, 1), proc(2, "b", 90.0, 2), proc(3, "c", 5.0, 3)], total_threads: None })
            .unwrap();
        a.pump();
        assert_eq!(a.view().sel[1], 0);
        assert_eq!(a.selected_proc().map(|p| p.pid), Some(2));
    }

    #[test]
    fn the_filter_types_and_escape_drops_it() {
        let (mut a, tx, _ctl) = app();
        tx.send(Msg::Procs { procs: vec![proc(1, "alpha", 0.0, 1), proc(2, "beta", 0.0, 2)], total_threads: None }).unwrap();
        a.pump();
        keys(&mut a, "/ b e");
        assert_eq!(a.screen, Screen::Procs);
        assert_eq!(a.mode, Mode::Filter);
        assert_eq!(a.rows.len(), 1);
        keys(&mut a, "<Backspace> <Backspace>");
        assert_eq!(a.rows.len(), 2);
        keys(&mut a, "x <Enter>");
        assert_eq!((a.mode, a.filter.as_str()), (Mode::Normal, "x"));
        keys(&mut a, "<Esc>");
        assert!(a.filter.is_empty());
    }

    #[test]
    fn keys_move_a_divider_and_say_when_there_is_none() {
        let (mut a, _tx, _ctl) = app();
        keys(&mut a, "2");
        let (right, up) = if cfg!(target_os = "macos") { ("<D-C-Right>", "<D-C-Up>") } else { ("<A-S-Right>", "<A-S-Up>") };
        let before = a.layouts[&Screen::Cpu].clone();
        keys(&mut a, right);
        assert_ne!(a.layouts[&Screen::Cpu], before);
        keys(&mut a, up);
        assert_eq!(a.toast.as_ref().map(|t| t.0.as_str()), Some(tr("toast.no_divider")));
        let ctrl = if cfg!(target_os = "macos") { "<D-S-e>" } else { "<C-S-e>" };
        keys(&mut a, ctrl);
        let w: Vec<f32> =
            a.layouts[&Screen::Cpu].layout(tsumugi_layout::Rect::new(0.0, 0.0, 300.0, 10.0), 0.0).iter().map(|(_, r)| r.w).collect();
        assert!(w.iter().all(|x| (x - 100.0).abs() < 0.5), "{w:?}");
    }

    #[test]
    fn interval_steps_and_pause_reach_the_sampler() {
        let (mut a, _tx, ctl) = app();
        keys(&mut a, "+ + -");
        assert_eq!(a.interval, 2.0);
        keys(&mut a, "P");
        let got: Vec<Ctl> = ctl.try_iter().collect();
        assert_eq!(got.len(), 4);
        assert!(matches!(got[3], Ctl::Pause(true)));
        for _ in 0..10 {
            keys(&mut a, "-");
        }
        assert_eq!(a.interval, 0.5);
    }

    #[test]
    fn tab_skips_screens_not_built_and_enter_opens_a_process() {
        let (mut a, tx, _ctl) = app();
        keys(&mut a, "<Tab>");
        assert_eq!(a.screen, Screen::Cpu);
        keys(&mut a, "<Tab>");
        assert_eq!(a.screen, Screen::Mem);
        keys(&mut a, "<Tab>");
        assert_eq!(a.screen, Screen::Gpu);
        keys(&mut a, "<Tab>");
        assert_eq!(a.screen, Screen::Procs);
        keys(&mut a, "<Tab>");
        assert_eq!(a.screen, Screen::Svc);
        keys(&mut a, "<Tab>");
        assert_eq!(a.screen, Screen::Net);
        keys(&mut a, "<Tab>");
        assert_eq!(a.screen, Screen::Ovw);
        tx.send(Msg::Procs { procs: vec![proc(7, "big", 80.0, 9), proc(8, "small", 1.0, 1)], total_threads: None }).unwrap();
        a.pump();
        keys(&mut a, "l l <Enter>");
        assert_eq!(a.screen, Screen::Procs);
        assert_eq!(a.selected_proc().map(|p| p.pid), Some(7));
    }

    #[test]
    fn t_shows_the_tree_and_u_switches_the_scope() {
        let (mut a, tx, _ctl) = app();
        let child = |pid, ppid, cpu, user: &str| Proc { ppid: Some(ppid), user: Some(user.into()), ..proc(pid, "c", cpu, 1) };
        let me = Proc { user: Some("me".into()), ..proc(std::process::id(), "mimamori", 0.0, 1) };
        let init = Proc { user: Some("root".into()), ..proc(1, "init", 0.0, 1) };
        let procs = vec![init, child(10, 1, 5.0, "root"), child(11, 1, 9.0, "me"), child(20, 10, 1.0, "me"), me];
        tx.send(Msg::Procs { procs, total_threads: None }).unwrap();
        a.pump();
        keys(&mut a, "5 t");
        assert!(a.tree);
        assert_eq!(a.toast.as_ref().map(|t| t.0.as_str()), Some(tr("toast.tree_on")));
        let pids: Vec<u32> = a.rows.iter().map(|&i| a.model.procs[i].pid).collect();
        // Siblings by CPU, as the list is sorted: 11 before 10.
        assert_eq!(&pids[..4], [1, 11, 10, 20]);
        assert_eq!(&a.tree_lines[..4], ["", "├ ", "└ ", "  └ "]);
        keys(&mut a, "u");
        assert!(a.mine_only());
        assert_eq!(a.toast.as_ref().map(|t| t.0.clone()), Some(trf("toast.scope", &[tr("procs.mine")])));
        // Neither 20's parent nor 11's is mine, so each is a root.
        let pids: Vec<u32> = a.rows.iter().map(|&i| a.model.procs[i].pid).collect();
        assert_eq!(pids.len(), 3);
        assert!(a.tree_lines.iter().all(|l| l.is_empty()), "{:?}", a.tree_lines);
        keys(&mut a, "u t");
        assert!(!a.mine_only() && !a.tree && a.tree_lines.is_empty());
        assert_eq!(a.rows.len(), 5);
    }

    #[test]
    fn x_asks_first_and_n_keeps_the_process() {
        let (mut a, tx, _ctl) = app();
        keys(&mut a, "x");
        assert_eq!((a.mode, a.toast.as_ref().map(|t| t.0.as_str())), (Mode::Normal, Some(tr("toast.kill_none"))));
        tx.send(Msg::Procs { procs: vec![proc(7, "big", 80.0, 9), proc(std::process::id(), "mimamori", 1.0, 1)], total_threads: None })
            .unwrap();
        a.pump();
        keys(&mut a, "5 l x");
        assert_eq!(a.mode, Mode::Confirm);
        assert_eq!(a.confirm.as_ref().map(|t| t.pid), Some(7));
        // Keys of the main layer do nothing while the question is up.
        keys(&mut a, "j 1");
        assert_eq!((a.screen, a.mode), (Screen::Procs, Mode::Confirm));
        keys(&mut a, "n");
        assert_eq!((a.mode, a.confirm.clone()), (Mode::Normal, None));
        keys(&mut a, "j x");
        assert_eq!(a.mode, Mode::Normal);
        assert_eq!(a.toast.as_ref().map(|t| t.0.as_str()), Some(tr("toast.kill_self")));
    }

    #[test]
    fn enter_on_a_gpu_and_then_its_process_opens_procs() {
        let (mut a, tx, _ctl) = app();
        tx.send(Msg::Procs { procs: vec![proc(7, "game", 80.0, 9), proc(8, "idle", 1.0, 1)], total_threads: None }).unwrap();
        let procs = vec![gpu::GpuProc { pid: 7, usage: Some(60.0), mem: Some(1 << 30) }];
        let dev = |id: &str, procs| gpu::Device { id: id.into(), usage: Some(60.0), procs, ..gpu::Device::default() };
        tx.send(Msg::Gpu(vec![dev("a", Vec::new()), dev("b", procs)])).unwrap();
        a.pump();
        keys(&mut a, "4 <Enter>");
        assert_eq!(a.focus(), Pane::Left, "nothing to open on a device without processes");
        keys(&mut a, "j <Enter>");
        assert_eq!((a.focus(), a.count(Screen::Gpu, Pane::Right)), (Pane::Right, 1));
        assert_eq!(a.focused_proc().map(|p| p.pid), Some(7));
        keys(&mut a, "<Enter>");
        assert_eq!(a.screen, Screen::Procs);
        assert_eq!(a.selected_proc().map(|p| p.pid), Some(7));
        // A device that goes away takes the selection back into the list.
        tx.send(Msg::Gpu(vec![dev("a", Vec::new())])).unwrap();
        a.pump();
        assert_eq!(a.view_of(Screen::Gpu).sel, [0, 0, 0]);
    }

    #[test]
    fn svc_reads_only_while_shown_and_enter_opens_its_process() {
        let (mut a, tx, ctl) = app();
        keys(&mut a, "6 6 5");
        let got: Vec<Ctl> = ctl.try_iter().collect();
        assert!(matches!(got[..], [Ctl::Services(true), Ctl::Services(false)]), "{got:?}");
        tx.send(Msg::Procs { procs: vec![proc(7, "sshd", 1.0, 1)], total_threads: None }).unwrap();
        let svc = |name: &str, state, pid| svc::Service { name: name.into(), state, pid, ..svc::Service::default() };
        let list =
            vec![svc("cron", svc::State::Running, None), svc("dead", svc::State::Failed, None), svc("sshd", svc::State::Running, Some(7))];
        tx.send(Msg::Svc(svc::Services { list, source: "systemd", note: None })).unwrap();
        a.pump();
        keys(&mut a, "6");
        assert_eq!((a.svc_rows.len(), a.count_svc(Some(svc::State::Failed))), (3, 1));
        // The left column's third row shows the stopped ones: none.
        keys(&mut a, "j j");
        assert!(a.svc_rows.is_empty());
        keys(&mut a, "k <Enter>");
        assert_eq!((a.focus(), a.svc_rows.len()), (Pane::Mid, 2));
        keys(&mut a, "<Enter>");
        assert_eq!(a.toast.as_ref().map(|t| t.0.clone()), Some(trf("toast.svc_no_pid", &["cron"])));
        // The filter narrows the list and the selection stays on the name.
        keys(&mut a, "j / s s <Enter>");
        assert_eq!((a.screen, a.svc_rows.len()), (Screen::Svc, 1));
        assert_eq!(a.selected_svc().map(|s| s.name.as_str()), Some("sshd"));
        keys(&mut a, "<Enter>");
        assert_eq!(a.screen, Screen::Procs);
        assert_eq!(a.selected_proc().map(|p| p.pid), Some(7));
        assert!(matches!(ctl.try_iter().last(), Some(Ctl::Services(false))));
    }

    #[test]
    fn net_reads_sockets_only_while_shown_and_enter_opens_the_owner() {
        let (mut a, tx, ctl) = app();
        keys(&mut a, "7 1");
        let got: Vec<Ctl> = ctl.try_iter().collect();
        assert!(matches!(got[..], [Ctl::Net(true), Ctl::Net(false)]), "{got:?}");
        tx.send(Msg::Procs { procs: vec![proc(7, "sshd", 1.0, 1)], total_threads: None }).unwrap();
        let conn = |proto, local: &str, remote: Option<&str>, state, pid| net::Conn {
            proto,
            local: local.parse().unwrap(),
            remote: remote.map(|r| r.parse().unwrap()),
            state,
            pid,
        };
        let list = vec![
            conn(net::Proto::Tcp, "0.0.0.0:22", None, net::State::Listen, Some(7)),
            conn(net::Proto::Tcp, "10.0.0.2:50000", Some("8.8.8.8:443"), net::State::Established, None),
            conn(net::Proto::Udp, "0.0.0.0:5353", None, net::State::None, None),
        ];
        tx.send(Msg::Conns(net::Conns { list, source: "/proc/net", note: None })).unwrap();
        tx.send(Msg::Nics(vec![net::Nic { name: "eth0".into(), up: true, ..net::Nic::default() }])).unwrap();
        a.pump();
        keys(&mut a, "7");
        assert_eq!((a.net_rows.len(), a.selected_nic().map(|n| n.name.as_str())), (3, Some("eth0")));
        // u keeps the sockets that wait for others: TCP listening and unconnected UDP.
        keys(&mut a, "u");
        assert!(a.net_listen);
        assert_eq!(a.net_rows.len(), 2);
        assert_eq!(a.toast.as_ref().map(|t| t.0.clone()), Some(trf("toast.scope", &[tr("net.listening")])));
        keys(&mut a, "u");
        assert_eq!(a.net_rows.len(), 3);
        // Enter on an interface moves to the sockets; one with no owner toasts.
        keys(&mut a, "<Enter> j <Enter>");
        assert_eq!(a.focus(), Pane::Mid);
        assert_eq!(a.toast.as_ref().map(|t| t.0.clone()), Some(trf("toast.net_no_pid", &["10.0.0.2:50000"])));
        // The filter matches the process name, and the selection follows the socket.
        keys(&mut a, "/ s s h <Enter>");
        assert_eq!((a.screen, a.net_rows.len()), (Screen::Net, 1));
        assert_eq!(a.selected_conn().map(|c| c.local.port()), Some(22));
        keys(&mut a, "<Enter>");
        assert_eq!(a.screen, Screen::Procs);
        assert_eq!(a.selected_proc().map(|p| p.pid), Some(7));
        assert!(matches!(ctl.try_iter().last(), Some(Ctl::Net(false))));
    }

    #[test]
    fn y_ends_it_on_a_thread_and_the_outcome_comes_back_as_a_toast() {
        let (mut a, tx, _ctl) = app();
        // A PID nothing holds: the worker finds it gone and sends nothing.
        tx.send(Msg::Procs { procs: vec![proc(9_999_999, "ghost", 1.0, 1)], total_threads: None }).unwrap();
        a.pump();
        keys(&mut a, "5 l x y");
        assert_eq!((a.mode, a.confirm.clone()), (Mode::Normal, None));
        let until = Instant::now() + Duration::from_secs(10);
        while a.toast.as_ref().is_some_and(|t| t.0.starts_with("Ending")) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
            a.pump();
        }
        assert_eq!(a.toast.map(|t| t.0), Some(trf("toast.gone", &["ghost", "9999999"])));
    }
}
