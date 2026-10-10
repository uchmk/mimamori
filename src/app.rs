//! The app's state and what each [`Act`] does. Drawing is in `ui/`; this
//! side owns the selection, the layouts and the sampler's channels.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use ito_layout::Node;

use crate::collect::{Ctl, Msg, Proc, disk, gpu, hw, net, sensors, svc};
use crate::config::cmd::{Act, Goto, Step};
use crate::config::keymap::{Keymap, Match, resolve};
use crate::config::keys::{Code, Key};
use crate::i18n::{tr, trf};
use crate::kill::{self, Outcome, Target};
use crate::model::Model;
use crate::persist::Saver;
use crate::prefs::{Prefs, Reload};
use crate::remote::client::{Event, ToHost};
use crate::remote::proto::{self, Done, Refusal, Want};
use crate::remote::{self, Link, Peer};
use crate::screen::{Pane, Screen};
use crate::svcctl::{self, Action};

/// The sampling intervals `+` and `-` step through, in seconds.
pub const INTERVALS: [f32; 6] = [0.5, 1.0, 2.0, 3.0, 5.0, 10.0];
/// How long a toast stays in the status bar.
pub const TOAST: Duration = Duration::from_secs(3);
/// How far a resize key moves a divider, as a share of the room.
const NUDGE: f32 = 0.03;
/// Rows the overview lists in each of its two top lists.
pub const OVW_TOP: usize = 5;

/// A row of the overview's left column.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gauge {
    Cpu,
    Mem,
    Swap,
    /// A GPU or NPU, by its index in the model's list.
    Gpu(usize),
    /// A drive, by its index in the model's list.
    Disk(usize),
}
const TOP_CPU: usize = 10;
const TOP_MEM: usize = 15;

/// What the confirm question asks.
#[derive(Clone, Debug, PartialEq)]
pub enum Ask {
    Kill(Target),
    Svc(svcctl::Target),
}

impl Ask {
    /// The process's or the service's name.
    pub fn name(&self) -> &str {
        match self {
            Ask::Kill(t) => &t.name,
            Ask::Svc(t) => &t.name,
        }
    }

    /// The `[agent] allow` word that lets another machine do it.
    pub fn word(&self) -> &'static str {
        match self {
            Ask::Kill(_) => "kill",
            Ask::Svc(_) => "service",
        }
    }

    /// The request for another machine's agent.
    fn act(&self) -> proto::Act {
        match self {
            Ask::Kill(t) => proto::Act::Kill { pid: t.pid, start: t.start },
            Ask::Svc(t) => proto::Act::Service { action: t.action, name: t.name.clone(), driver: t.driver },
        }
    }

    /// The toast while it runs.
    fn starting(&self) -> String {
        match self {
            Ask::Kill(t) => trf("toast.ending", &[&t.name, &t.pid.to_string()]),
            Ask::Svc(t) => {
                let key = match t.action {
                    Action::Start => "toast.svc_starting",
                    Action::Stop => "toast.svc_stopping",
                    Action::Restart => "toast.svc_restarting",
                };
                trf(key, &[&t.name])
            }
        }
    }
}

/// A request sent to another machine's agent, until it answers.
#[derive(Clone, Debug, PartialEq)]
pub struct Asked {
    pub id: u64,
    /// The machine, by its index in `peers`.
    pub peer: usize,
    pub ask: Ask,
}

/// The kind of OS a machine runs, for the words that differ by it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Windows,
    Mac,
    Other,
}

impl Family {
    pub fn here() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Mac
        } else {
            Self::Other
        }
    }

    /// From a machine's OS as its hello says it (`Windows 11 Pro`,
    /// `macOS 15 Sequoia`, `Linux 24.04 Ubuntu`).
    pub fn of(os: &str) -> Self {
        let os = os.to_lowercase();
        if os.contains("windows") {
            Self::Windows
        } else if os.contains("mac") || os.contains("darwin") {
            Self::Mac
        } else {
            Self::Other
        }
    }
}

/// The toast once a process has been ended, or not.
pub fn kill_toast(t: &Target, o: Outcome) -> String {
    let (name, pid) = (t.name.as_str(), t.pid.to_string());
    match o {
        Outcome::Ended => trf("toast.ended", &[name, &pid]),
        Outcome::Killed => trf("toast.killed", &[name, &pid]),
        Outcome::Gone => trf("toast.gone", &[name, &pid]),
        Outcome::Failed(e) => trf("toast.kill_failed", &[name, &pid, &e]),
    }
}

/// The toast once a service has moved, or not, on a machine of `family`.
pub fn svc_toast(t: &svcctl::Target, o: svcctl::Outcome, family: Family) -> String {
    let n = t.name.as_str();
    match (o, t.action) {
        (svcctl::Outcome::Done, Action::Start) => trf("toast.svc_started", &[n]),
        (svcctl::Outcome::Done, Action::Stop) => trf("toast.svc_stopped", &[n]),
        (svcctl::Outcome::Done, Action::Restart) => trf("toast.svc_restarted", &[n]),
        (svcctl::Outcome::Denied, _) if family == Family::Windows => trf("toast.svc_denied_win", &[n]),
        (svcctl::Outcome::Denied, _) if family == Family::Mac => trf("toast.svc_denied_mac", &[n]),
        (svcctl::Outcome::Denied, _) => trf("toast.svc_denied_linux", &[n]),
        (svcctl::Outcome::Failed(e), Action::Start) => trf("toast.svc_start_failed", &[n, &e]),
        (svcctl::Outcome::Failed(e), Action::Stop) => trf("toast.svc_stop_failed", &[n, &e]),
        (svcctl::Outcome::Failed(e), Action::Restart) => trf("toast.svc_restart_failed", &[n, &e]),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Filter,
    Help,
    /// The question before a process is ended.
    Confirm,
    /// The settings screen (`Ctrl+,`), across the whole window.
    Settings,
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

/// Inside another machine's screens: what is kept of this one's meanwhile.
pub struct Away {
    /// The machine, by its index in `peers`.
    pub peer: usize,
    /// This machine's model, still fed by its sampler.
    pub home: Model,
    home_views: HashMap<Screen, View>,
    home_me: Option<String>,
    /// The connection dropped while inside; the last readings stay.
    pub lost: bool,
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
    pub moved: Option<ito_layout::ui::Moved<Pane>>,
    /// How far the help can scroll, so a key past the end stays there.
    pub help_max: Option<usize>,
    /// The fleet tab clicked.
    pub fleet_tab: bool,
    /// Cards in a row of the fleet, for the up and down keys.
    pub fleet_cols: Option<usize>,
    /// A fleet card clicked (0 is this machine), and whether twice.
    pub fleet_click: Option<(usize, bool)>,
    /// A line of the fleet's hint clicked: what it copies.
    pub copy: Option<String>,
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
    /// svc shows Windows' drivers instead of its services (`u`).
    pub svc_drivers: bool,
    /// The socket list as net shows it: indices into `model.conns.list`.
    pub net_rows: Vec<usize>,
    /// The socket selected, kept while the list is read again.
    net_key: Option<ConnKey>,
    /// net lists only the sockets waiting for others (`u`).
    pub net_listen: bool,
    /// net lists only the sockets on this interface (`Enter` on it, `Esc`
    /// drops it); it follows the selection while set.
    pub net_nic: Option<String>,
    me: Option<String>,
    dirty: bool,
    pub interval: f32,
    pub paused: bool,
    pub toast: Option<(String, Instant)>,
    pub help_scroll: usize,
    /// What the confirm question is about.
    pub confirm: Option<Ask>,
    kill_tx: Sender<(Target, Outcome)>,
    kill_rx: Receiver<(Target, Outcome)>,
    svc_tx: Sender<(svcctl::Target, svcctl::Outcome)>,
    svc_rx: Receiver<(svcctl::Target, svcctl::Outcome)>,
    /// Repainted when a process has been ended or a service has moved, so
    /// the toast shows at once.
    pub wake: Option<egui::Context>,
    saver: Saver,
    pub quit: bool,
    wheel_acc: f32,
    /// The fleet shows over the screen (`F`).
    pub fleet: bool,
    /// The `[[host]]` machines; card 0 is this one, card `i` is `peers[i - 1]`.
    pub peers: Vec<Peer>,
    fleet_rx: Option<Receiver<(usize, Event)>>,
    /// What tells each host's watcher what to send and what to ask, by its
    /// index in `peers`.
    fleet_tx: Vec<Sender<ToHost>>,
    /// Requests sent to other machines' agents and not yet answered.
    pub asked: Vec<Asked>,
    next_act: u64,
    /// What each host was last told to send.
    sent_remote: Vec<Want>,
    /// Whether this machine's sampler was last told to read services and
    /// sockets.
    sent_local: (bool, bool),
    /// Inside another machine's screens (`Enter` on its card, `[` / `]`).
    pub away: Option<Away>,
    /// The card selected.
    pub fleet_sel: usize,
    /// Cards in a row, as last drawn.
    pub fleet_cols: usize,
    /// The settings files and the settings screen's state.
    pub prefs: Prefs,
    /// Words to put on the clipboard this frame.
    pub copy: Option<String>,
}

impl App {
    pub fn new(km: Keymap, rx: Receiver<Msg>, ctl: Sender<Ctl>, layouts: HashMap<Screen, Node<Pane>>, saver: Saver, interval: f32) -> Self {
        let (kill_tx, kill_rx) = crossbeam_channel::unbounded();
        let (svc_tx, svc_rx) = crossbeam_channel::unbounded();
        Self {
            km,
            model: Model::default(),
            rx,
            ctl,
            screen: Screen::Ovw,
            layouts,
            drag: None,
            views: new_views(),
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
            svc_drivers: false,
            net_rows: Vec::new(),
            net_key: None,
            net_listen: false,
            net_nic: None,
            me: None,
            dirty: true,
            interval,
            paused: false,
            toast: None,
            help_scroll: 0,
            confirm: None,
            kill_tx,
            kill_rx,
            svc_tx,
            svc_rx,
            wake: None,
            saver,
            quit: false,
            wheel_acc: 0.0,
            fleet: false,
            peers: Vec::new(),
            fleet_rx: None,
            fleet_tx: Vec::new(),
            asked: Vec::new(),
            next_act: 0,
            sent_remote: Vec::new(),
            sent_local: (false, false),
            away: None,
            fleet_sel: 0,
            fleet_cols: 1,
            prefs: Prefs::empty(),
            copy: None,
        }
    }

    /// The `[[host]]` machines, their watchers' events and what tells each
    /// watcher what to send.
    pub fn set_fleet(&mut self, peers: Vec<Peer>, (rx, tx): (Receiver<(usize, Event)>, Vec<Sender<ToHost>>)) {
        self.sent_remote = vec![Want::default(); peers.len()];
        self.peers = peers;
        self.fleet_rx = Some(rx);
        self.fleet_tx = tx;
    }

    /// This machine's model, also while inside another's.
    pub fn home(&self) -> &Model {
        self.away.as_ref().map_or(&self.model, |a| &a.home)
    }

    /// The machine whose screens show, unless it is this one.
    pub fn away_peer(&self) -> Option<&Peer> {
        self.away.as_ref().and_then(|a| self.peers.get(a.peer))
    }

    /// The kind of OS of the machine whose screens show.
    pub fn family(&self) -> Family {
        match self.away_peer() {
            Some(p) => Family::of(p.hello.as_ref().map_or("", |h| h.os.as_str())),
            None => Family::here(),
        }
    }

    /// The fleet card selected, unless it is this machine's.
    pub fn selected_peer(&self) -> Option<&Peer> {
        self.fleet_sel.checked_sub(1).and_then(|i| self.peers.get(i))
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

    /// Put `text` on the clipboard (in the frame) and say so.
    pub fn copy(&mut self, text: String) {
        self.toast(trf("toast.copied", &[text.lines().next().unwrap_or("")]));
        self.copy = Some(text);
    }

    /// Sample every `s` seconds from now on.
    pub fn set_interval(&mut self, s: f32) {
        self.interval = s;
        let _ = self.ctl.send(Ctl::Interval(Duration::from_secs_f32(s)));
    }

    pub fn open_settings(&mut self) {
        self.mode = Mode::Settings;
        self.pending.clear();
    }

    pub fn close_settings(&mut self) {
        self.mode = Mode::Normal;
    }

    /// What a key does on the settings screen, from its layer of the keymap.
    pub fn settings_act(&self, k: Key) -> Option<Act> {
        match resolve(&self.km.settings, &[k]) {
            Match::Exact(b) => b.run.first().copied(),
            _ => None,
        }
    }

    /// The language as config.toml, common.toml and the OS pick it now.
    fn relanguage(&self) {
        let os = ito_i18n::os_language();
        let app = self.prefs.settings.app_language.as_deref();
        crate::i18n::set_language(&crate::i18n::resolve_language(app, self.prefs.common.language.as_deref(), os.as_deref()));
    }

    /// Fold in a settings file read again.
    fn reload(&mut self, r: Reload) {
        match r {
            Reload::Common(Ok((c, w))) => {
                self.prefs.common = c;
                self.prefs.common_warn = w;
                self.relanguage();
            }
            Reload::Common(Err(e)) => {
                self.toast(e.clone());
                self.prefs.common_warn = vec![e];
            }
            Reload::Themes(themes, w) => {
                self.prefs.themes = themes;
                self.prefs.theme_warn = w;
            }
            Reload::Settings(s) => {
                let old = std::mem::replace(&mut self.prefs.settings, *s);
                let interval = self.prefs.settings.interval;
                if interval != old.interval {
                    self.set_interval(interval);
                }
                if self.prefs.settings.hosts != old.hosts {
                    // The machine inside may be gone or another now.
                    self.go_home();
                    let hosts = self.prefs.settings.hosts.clone();
                    let fleet = remote::client::spawn(&hosts, self.wake.clone());
                    self.set_fleet(hosts.iter().map(Peer::new).collect(), fleet);
                    self.fleet_sel = self.fleet_sel.min(self.peers.len());
                }
                self.relanguage();
            }
            Reload::Failed(e) => self.toast(trf("toast.save_failed", &[&e])),
        }
    }

    /// Fold in what the sampler sent and bring the lists up to date.
    pub fn pump(&mut self) {
        let mut dirty = (false, false);
        while let Ok(m) = self.rx.try_recv() {
            match &mut self.away {
                // Kept up to date for the way back and this machine's card.
                Some(a) => a.home.apply(m),
                None => self.take(m, &mut dirty),
            }
        }
        if let Some(rx) = &self.fleet_rx {
            let events: Vec<(usize, Event)> = rx.try_iter().collect();
            for (i, e) in events {
                self.peer_event(i, e, &mut dirty);
            }
        }
        let (svc_dirty, net_dirty) = dirty;
        if svc_dirty {
            self.refresh_svc();
        }
        if net_dirty && self.model.conn_read {
            self.refresh_net();
        }
        while let Ok((t, o)) = self.kill_rx.try_recv() {
            self.toast(kill_toast(&t, o));
        }
        while let Ok((t, o)) = self.svc_rx.try_recv() {
            self.toast(svc_toast(&t, o, Family::here()));
            // Read the list again now rather than at the next interval;
            // not while inside another machine, whose agent does that.
            if self.screen == Screen::Svc && self.away.is_none() {
                let _ = self.ctl.send(Ctl::Services(true));
            }
        }
        while let Ok(r) = self.prefs.rx.try_recv() {
            self.reload(r);
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

    /// Fold a reading into the screens' model, noting the lists it changes
    /// (services, sockets).
    fn take(&mut self, m: Msg, (svc, net): &mut (bool, bool)) {
        match m {
            // The net filter matches process names too.
            Msg::Procs { .. } => (self.dirty, *net) = (true, true),
            Msg::Svc(_) => *svc = true,
            Msg::Conns(_) => *net = true,
            // An interface's addresses may change under its narrowing.
            Msg::Nics(_) if self.net_nic.is_some() => *net = true,
            _ => {}
        }
        self.model.apply(m);
    }

    /// News from a host's watcher: its card's, and its readings while its
    /// screens show.
    fn peer_event(&mut self, i: usize, e: Event, dirty: &mut (bool, bool)) {
        let inside = self.away.as_ref().is_some_and(|a| a.peer == i);
        match e {
            Event::Msg(m) if inside => self.take(*m, dirty),
            Event::Msg(_) => {}
            Event::Done(id, d) => self.answered(i, id, d),
            e => {
                let (down, up) = (matches!(e, Event::Down(_)), matches!(e, Event::Up(_)));
                let Some(p) = self.peers.get_mut(i) else { return };
                p.apply(e);
                let name = p.name.clone();
                if let Some(a) = self.away.as_mut().filter(|_| inside) {
                    if down && !a.lost {
                        a.lost = true;
                        self.toast(trf("toast.remote_lost", &[&name]));
                    } else if up && a.lost {
                        // The watcher says again what it wants.
                        a.lost = false;
                        self.toast(trf("toast.machine", &[&name]));
                    }
                }
                // No answer comes over the next connection.
                let asked = self.asked.len();
                self.asked.retain(|a| !down || a.peer != i);
                if self.asked.len() < asked {
                    self.toast(trf("toast.remote_no_answer", &[&name]));
                }
            }
        }
    }

    /// The answer to request `id` sent to `peers[i]`.
    fn answered(&mut self, i: usize, id: u64, d: Done) {
        let Some(at) = self.asked.iter().position(|a| a.id == id && a.peer == i) else { return };
        let ask = self.asked.remove(at).ask;
        let Some(p) = self.peers.get(i) else { return };
        let name = p.name.clone();
        let family = Family::of(p.hello.as_ref().map_or("", |h| h.os.as_str()));
        let on = |s: String| trf("toast.on_machine", &[&name, &s]);
        let s = match (d, &ask) {
            (Done::Kill(o), Ask::Kill(t)) => on(kill_toast(t, o)),
            (Done::Svc(o), Ask::Svc(t)) => on(svc_toast(t, o, family)),
            (Done::Refused(Refusal::NotAllowed), _) => trf("toast.fleet_view_only", &[&name, ask.word()]),
            (Done::Refused(Refusal::Agent), _) => trf("toast.kill_agent", &[&name]),
            (Done::Refused(Refusal::NoService), _) => on(trf("toast.remote_no_service", &[ask.name()])),
            (Done::Refused(Refusal::Disabled(line)), _) => on(trf("toast.svc_disabled", &[ask.name(), &line])),
            // An answer of the other kind is not this request's.
            (Done::Kill(_) | Done::Svc(_), _) => return,
        };
        self.toast(s);
    }

    /// Send a confirmed question to the agent of the machine whose screens
    /// show.
    fn send_act(&mut self, ask: Ask) {
        let Some(i) = self.away.as_ref().map(|a| a.peer) else { return };
        let Some(p) = self.peers.get(i) else { return };
        let name = p.name.clone();
        self.next_act += 1;
        let id = self.next_act;
        let sent = p.link == Link::Up && self.fleet_tx.get(i).is_some_and(|tx| tx.send(ToHost::Act(id, ask.act())).is_ok());
        if !sent {
            self.toast(trf("toast.fleet_down", &[&name]));
            return;
        }
        self.toast(trf("toast.on_machine", &[&name, &ask.starting()]));
        self.asked.push(Asked { id, peer: i, ask });
    }

    /// Show another machine's screens: card `card` of the fleet (0 is this
    /// one).
    pub fn enter_machine(&mut self, card: usize) {
        let Some(i) = card.checked_sub(1) else {
            self.go_home();
            return;
        };
        let Some(p) = self.peers.get(i) else { return };
        if p.link != Link::Up {
            self.toast(trf("toast.fleet_down", &[&p.name]));
            return;
        }
        let (name, user) = (p.name.clone(), p.hello.as_ref().map(|h| h.user.clone()).filter(|u| !u.is_empty()));
        let (home, home_views, home_me) = match self.away.take() {
            Some(a) => (a.home, a.home_views, a.home_me),
            None => (std::mem::take(&mut self.model), std::mem::replace(&mut self.views, new_views()), self.me.take()),
        };
        self.away = Some(Away { peer: i, home, home_views, home_me, lost: false });
        self.model = Model::default();
        self.views = new_views();
        self.me = user;
        self.fresh_lists();
        self.fleet = false;
        self.fleet_sel = card;
        self.sync_sources();
        self.toast(trf("toast.machine", &[&name]));
    }

    /// Back to this machine's screens, where they were left.
    pub fn go_home(&mut self) {
        let Some(a) = self.away.take() else { return };
        self.model = a.home;
        self.views = a.home_views;
        self.me = a.home_me;
        self.fresh_lists();
        self.fleet = false;
        self.fleet_sel = 0;
        self.sync_sources();
        self.toast(tr("toast.machine_home"));
    }

    /// The next (`d` > 0) or previous machine: this one, then the hosts that
    /// are up, round.
    fn cycle(&mut self, d: i32) {
        let cur = self.away.as_ref().map_or(0, |a| a.peer + 1);
        let stops: Vec<usize> =
            std::iter::once(0).chain((1..=self.peers.len()).filter(|&c| c == cur || self.peers[c - 1].link == Link::Up)).collect();
        if stops.len() < 2 {
            self.toast(tr("toast.machine_none"));
            return;
        }
        let at = stops.iter().position(|&c| c == cur).unwrap_or(0) as i32;
        let n = stops.len() as i32;
        self.enter_machine(stops[(at + d).rem_euclid(n) as usize]);
    }

    /// Lists, selections and the filter start over for another model.
    fn fresh_lists(&mut self) {
        self.filter.clear();
        if self.mode == Mode::Filter {
            self.mode = Mode::Normal;
        }
        self.proc_pid = None;
        self.svc_name = None;
        self.net_key = None;
        self.net_nic = None;
        self.drag = None;
        self.pending.clear();
        self.rows.clear();
        self.tree_lines.clear();
        self.top_cpu.clear();
        self.top_mem.clear();
        self.svc_rows.clear();
        self.net_rows.clear();
        self.dirty = true;
        self.refresh_svc();
        self.refresh_net();
    }

    /// Tell the samplers what the screen shown needs: services and sockets
    /// only while their screen shows, and another machine's readings only
    /// while inside it.
    fn sync_sources(&mut self) {
        let (svc, net) = (self.screen == Screen::Svc, self.screen == Screen::Net);
        let away = self.away.as_ref().map(|a| a.peer);
        let local = if away.is_some() { (false, false) } else { (svc, net) };
        if local.0 != self.sent_local.0 {
            let _ = self.ctl.send(Ctl::Services(local.0));
        }
        if local.1 != self.sent_local.1 {
            let _ = self.ctl.send(Ctl::Net(local.1));
        }
        self.sent_local = local;
        for (i, tx) in self.fleet_tx.iter().enumerate() {
            let want = if away == Some(i) { Want { detail: true, services: svc, conns: net } } else { Want::default() };
            if self.sent_remote.get(i) != Some(&want) {
                let _ = tx.send(ToHost::Want(want));
                if let Some(w) = self.sent_remote.get_mut(i) {
                    *w = want;
                }
            }
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

    fn svc_matches(&self, s: &svc::Service, state: Option<svc::State>, f: &str) -> bool {
        s.driver == self.svc_drivers
            && state.is_none_or(|st| s.state == st)
            && (f.is_empty() || s.name.to_lowercase().contains(f) || s.display.to_lowercase().contains(f))
    }

    /// The services, or the drivers, svc shows: before the state and filter.
    pub fn svc_kind(&self) -> impl Iterator<Item = &svc::Service> {
        self.model.services.list.iter().filter(|s| s.driver == self.svc_drivers)
    }

    /// How many services a row of svc's left column would show.
    pub fn count_svc(&self, state: Option<svc::State>) -> usize {
        let f = self.filter.to_lowercase();
        self.model.services.list.iter().filter(|s| self.svc_matches(s, state, &f)).count()
    }

    fn refresh_svc(&mut self) {
        let f = self.filter.to_lowercase();
        let state = Self::SVC_SHOW[self.view_of(Screen::Svc).sel[0].min(3)];
        let list = &self.model.services.list;
        self.svc_rows = (0..list.len()).filter(|&i| self.svc_matches(&list[i], state, &f)).collect();
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

    /// The drive the disk screen has selected.
    pub fn selected_drive(&self) -> Option<&disk::Drive> {
        self.model.disks.drives.get(self.view_of(Screen::Disk).sel[0])
    }

    /// The volume the disk screen has selected.
    pub fn selected_volume(&self) -> Option<&disk::Volume> {
        self.model.disks.volumes.get(self.view_of(Screen::Disk).sel[1])
    }

    /// The chip the sensors screen has selected.
    pub fn selected_chip(&self) -> Option<sensors::Chip<'_>> {
        self.model.sensors.chips().into_iter().nth(self.view_of(Screen::Sensors).sel[0])
    }

    /// The sensor the sensors screen has selected.
    pub fn selected_sensor(&self) -> Option<&sensors::Sensor> {
        self.model.sensors.list.get(self.view_of(Screen::Sensors).sel[1])
    }

    /// The memory slot the sys screen has selected.
    pub fn selected_slot(&self) -> Option<&hw::Slot> {
        self.model.hw.as_ref()?.slots.get(self.view_of(Screen::Sys).sel[1])
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
        if let Some(name) = &self.net_nic
            && !net::on_nic(c, self.model.nics.iter().find(|n| &n.name == name))
        {
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
            (Screen::Ovw, Pane::Left) => 3 + self.model.gpus.len() + self.model.disks.drives.len(),
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
            (Screen::Disk, Pane::Left) => self.model.disks.drives.len(),
            (Screen::Disk, Pane::Mid) => self.model.disks.volumes.len(),
            (Screen::Sensors, Pane::Left) => self.model.sensors.chips().len(),
            (Screen::Sensors, Pane::Mid) => self.model.sensors.list.len(),
            (Screen::Sys, Pane::Left) => crate::ui::sys::lines(self).len(),
            (Screen::Sys, Pane::Mid) => self.model.hw.as_ref().map_or(0, |h| h.slots.len()),
            _ => 0,
        }
    }

    /// The overview's left column: CPU, memory and swap, then the GPUs and
    /// NPUs, then the drives.
    pub fn ovw_gauges(&self) -> Vec<Gauge> {
        let mut g = vec![Gauge::Cpu, Gauge::Mem, Gauge::Swap];
        g.extend((0..self.model.gpus.len()).map(Gauge::Gpu));
        g.extend((0..self.model.disks.drives.len()).map(Gauge::Disk));
        g
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
        if out.fleet_tab {
            self.fleet = true;
        }
        if let Some(text) = out.copy.take() {
            self.copy(text);
        }
        if let Some(c) = out.fleet_cols {
            self.fleet_cols = c.max(1);
        }
        if let Some((i, double)) = out.fleet_click {
            self.fleet_sel = i.min(self.peers.len());
            if double {
                self.run(Act::Enter);
            }
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
            (Screen::Net, Pane::Left) if self.net_nic.is_some() => {
                self.net_nic = self.selected_nic().map(|n| n.name.clone());
                self.refresh_net();
            }
            _ => {}
        }
    }

    pub fn goto(&mut self, s: Screen) {
        self.screen = s;
        self.drag = None;
        self.pending.clear();
        self.fleet = false;
        self.sync_sources();
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
            // The window takes the keys from egui itself (`main.rs`), since
            // the controls want the rest; this is the keys alone.
            Mode::Settings => {
                if let Some(Act::Escape | Act::Settings) = self.settings_act(k) {
                    self.close_settings();
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
                let Some(ask) = self.confirm.take() else { return };
                if self.away.is_some() {
                    self.send_act(ask);
                    return;
                }
                self.toast(ask.starting());
                match ask {
                    Ask::Kill(t) => kill::spawn(t, self.kill_tx.clone(), self.wake.clone()),
                    Ask::Svc(t) => svcctl::spawn(t, self.svc_tx.clone(), self.wake.clone()),
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
        let t = Target { pid: p.pid, name: p.name.clone(), start: p.start };
        // Inside another machine, its agent is the one not to end.
        match self.away_peer() {
            Some(m) if m.hello.as_ref().is_some_and(|h| h.pid == t.pid) => {
                let s = trf("toast.kill_agent", &[&m.name]);
                self.toast(s);
                return;
            }
            None if t.pid == std::process::id() => {
                self.toast(tr("toast.kill_self"));
                return;
            }
            _ => {}
        }
        self.confirm = Some(Ask::Kill(t));
        self.mode = Mode::Confirm;
    }

    /// Ask before starting, stopping or restarting the service under the
    /// cursor; one already where it would go only gets a toast.
    fn ask_svc(&mut self, action: Action) {
        let Some(s) = self.selected_svc() else {
            self.toast(tr("toast.svc_none"));
            return;
        };
        let name = s.name.clone();
        if !svcctl::fits(action, s.state) {
            let key = if action == Action::Start { "toast.svc_already_running" } else { "toast.svc_not_running" };
            self.toast(trf(key, &[&name]));
            return;
        }
        // A launchd job that is not loaded: loaded from its plist, unless it
        // is disabled, which is the owner's to undo. Another machine's agent
        // decides that from its own list.
        let mut plist = None;
        if s.unloaded && action != Action::Stop && self.away.is_none() {
            if s.start == svc::Start::Disabled {
                self.toast(trf("toast.svc_disabled", &[&name, &svcctl::enable_line(&name)]));
                return;
            }
            plist = s.file.clone();
        }
        let driver = s.driver;
        self.confirm = Some(Ask::Svc(svcctl::Target { action, name, plist, driver }));
        self.mode = Mode::Confirm;
    }

    /// The keys while the fleet shows; false for those that work as on
    /// any screen.
    fn run_fleet(&mut self, a: Act) -> bool {
        let last = self.peers.len() as i64;
        let (sel, cols) = (self.fleet_sel as i64, self.fleet_cols.max(1) as i64);
        let to = |i: i64| i.clamp(0, last) as usize;
        match a {
            Act::Arrow(Step::By(d)) => self.fleet_sel = to(sel + d * cols),
            Act::Arrow(Step::Page(f)) => self.fleet_sel = to(sel + if f < 0.0 { -cols } else { cols }),
            Act::Arrow(Step::Top) => self.fleet_sel = 0,
            Act::Arrow(Step::Bot) => self.fleet_sel = last as usize,
            Act::Focus(d) => self.fleet_sel = to(sel + d as i64),
            // The screens behind, when they are the card's machine's.
            Act::Enter if self.fleet_sel == self.away.as_ref().map_or(0, |a| a.peer + 1) => self.fleet = false,
            Act::Enter => self.enter_machine(self.fleet_sel),
            Act::Fleet | Act::Escape => self.fleet = false,
            Act::Copy => match self.selected_peer() {
                None if self.peers.is_empty() => self.copy(remote::host_sample()),
                None => self.toast(tr("toast.copy_none")),
                Some(p) => {
                    let addr = p.addr.clone();
                    self.copy(addr);
                }
            },
            Act::Kill | Act::Service(_) => {
                let what = if a == Act::Kill { "kill" } else { "service" };
                let t = match self.selected_peer() {
                    None => tr("toast.fleet_local").to_string(),
                    Some(p) if p.hello.as_ref().is_some_and(|h| proto::allowed(&h.allow, what)) => {
                        trf("toast.fleet_act_inside", &[&p.name])
                    }
                    Some(p) => trf("toast.fleet_view_only", &[&p.name, what]),
                };
                self.toast(t);
            }
            Act::Sort | Act::Tree | Act::Scope(_) | Act::Resize(_) | Act::Equalize | Act::Backspace => {}
            Act::Screen(_) | Act::Filter => {
                self.fleet = false;
                return false;
            }
            _ => return false,
        }
        true
    }

    pub fn run(&mut self, a: Act) {
        if self.fleet && self.run_fleet(a) {
            return;
        }
        let built = self.screen.built();
        let away = self.away_peer().map(|p| p.name.clone());
        // Ending a process or moving a service on another machine, which its
        // `[agent] allow` may not let.
        let refused = self.away_peer().and_then(|p| {
            let word = match a {
                Act::Kill if self.screen != Screen::Svc => "kill",
                Act::Kill | Act::Service(_) if self.screen == Screen::Svc => "service",
                _ => return None,
            };
            let allowed = p.hello.as_ref().is_some_and(|h| proto::allowed(&h.allow, word));
            (!allowed).then(|| trf("toast.fleet_view_only", &[&p.name, word]))
        });
        match a {
            Act::Machine(d) => self.cycle(d),
            Act::Kill | Act::Service(_) if refused.is_some() => self.toast(refused.unwrap_or_default()),
            // Its sampling is its own.
            Act::Pause | Act::Interval(_) if away.is_some() => self.toast(trf("toast.remote_sampling", &[&away.unwrap_or_default()])),
            Act::Fleet => {
                self.fleet = true;
                self.fleet_sel = self.fleet_sel.min(self.peers.len());
            }
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
            Act::Kill if self.screen == Screen::Svc => self.ask_svc(Action::Stop),
            Act::Kill => self.ask_kill(),
            Act::Service(action) if self.screen == Screen::Svc => self.ask_svc(action),
            Act::Service(_) => self.toast(tr("toast.svc_elsewhere")),
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
            Act::Scope(to) if self.screen == Screen::Svc => {
                let drivers = to.unwrap_or(!self.svc_drivers);
                if drivers && !self.model.services.list.iter().any(|s| s.driver) {
                    self.toast(tr("toast.svc_no_drivers"));
                    return;
                }
                self.svc_drivers = drivers;
                self.views.entry(Screen::Svc).or_default().sel[1] = 0;
                self.refresh_svc();
                self.toast(trf("toast.scope", &[tr(if drivers { "svc.drivers" } else { "svc.services" })]));
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
                self.set_interval(INTERVALS[i]);
                self.toast(trf("toast.interval", &[&fmt_interval(self.interval)]));
            }
            Act::Help => {
                self.mode = Mode::Help;
                self.help_scroll = 0;
            }
            Act::Settings => self.open_settings(),
            Act::Copy => self.toast(tr("toast.copy_none")),
            Act::Quit => self.quit = true,
            Act::Escape => {
                let nic = self.screen == Screen::Net && self.net_nic.take().is_some();
                if self.mode == Mode::Filter || !self.filter.is_empty() || nic {
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
            (Screen::Ovw, Pane::Left) => {
                let (s, row) = match self.ovw_gauges().get(sel) {
                    Some(Gauge::Cpu) | None => (Screen::Cpu, None),
                    Some(Gauge::Mem | Gauge::Swap) => (Screen::Mem, None),
                    Some(&Gauge::Gpu(i)) => (Screen::Gpu, Some(i)),
                    Some(&Gauge::Disk(i)) => (Screen::Disk, Some(i)),
                };
                self.goto(s);
                if let Some(i) = row {
                    let v = self.view_mut();
                    v.focus = Pane::Left.index();
                    v.sel[Pane::Left.index()] = i;
                }
            }
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
            (Screen::Net, Pane::Left) => {
                if let Some(name) = self.selected_nic().map(|n| n.name.clone()) {
                    self.toast(trf("toast.net_nic", &[&name]));
                    self.net_nic = Some(name);
                    self.refresh_net();
                }
                self.view_mut().focus = Pane::Mid.index();
            }
            (Screen::Net, Pane::Mid | Pane::Right) => match self.selected_conn().map(|c| (c.pid, c.local)) {
                Some((Some(pid), _)) => self.open_proc(pid),
                Some((None, local)) => self.toast(trf("toast.net_no_pid", &[&local.to_string()])),
                None => {}
            },
            // A drive: over to its first volume. A volume: back to its drive.
            (Screen::Disk, Pane::Left) => {
                let d = &self.model.disks;
                let id = self.selected_drive().map(|x| x.id.as_str());
                let first = d.volumes.iter().position(|v| v.drive.is_some() && v.drive.as_deref() == id);
                let v = self.view_mut();
                if let Some(i) = first {
                    v.sel[Pane::Mid.index()] = i;
                }
                v.focus = Pane::Mid.index();
            }
            (Screen::Disk, Pane::Mid) => {
                let d = &self.model.disks;
                let id = self.selected_volume().and_then(|v| v.drive.as_deref());
                if let Some(i) = d.drives.iter().position(|x| Some(x.id.as_str()) == id) {
                    let v = self.view_mut();
                    v.sel[Pane::Left.index()] = i;
                    v.focus = Pane::Left.index();
                }
            }
            // A chip: over to its first sensor. A sensor: back to its chip.
            (Screen::Sensors, Pane::Left) => {
                let first = self.selected_chip().map(|c| c.first);
                let v = self.view_mut();
                if let Some(i) = first {
                    v.sel[Pane::Mid.index()] = i;
                }
                v.focus = Pane::Mid.index();
            }
            (Screen::Sensors, Pane::Mid) => {
                let sel = self.view_of(Screen::Sensors).sel[Pane::Mid.index()];
                if let Some(i) = self.model.sensors.chips().iter().position(|c| (c.first..c.first + c.len).contains(&sel)) {
                    let v = self.view_mut();
                    v.sel[Pane::Left.index()] = i;
                    v.focus = Pane::Left.index();
                }
            }
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
    pub fn moved(&mut self, m: ito_layout::ui::Moved<Pane>) {
        use ito_layout::ui::Moved;
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
        s += &format!("screen: {}\n", if self.fleet { "fleet" } else { self.screen.name() });
        match (&self.away, self.away_peer()) {
            (Some(a), Some(p)) => s += &format!("machine: {}{}\n", p.name, if a.lost { " (lost)" } else { "" }),
            _ => s += "machine: this\n",
        }
        if self.fleet {
            let now = Instant::now();
            let hosts: Vec<String> = self.peers.iter().map(|p| format!("{}={}", p.name, p.status(now).key())).collect();
            s += &format!("fleet: sel {} of {}; {}\n", self.fleet_sel, self.peers.len() + 1, hosts.join(" "));
        }
        s += &format!("mode: {:?}\n", self.mode);
        if self.mode == Mode::Settings {
            s += &format!("settings: page {}, theme {}\n", self.prefs.state.page, self.prefs.common.theme_choice(None).0);
        }
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
            let kind = if self.svc_drivers { "drivers" } else { "services" };
            s += &format!("svcs: {} of {} ({}, {kind})\n", self.svc_rows.len(), sv.list.len(), sv.source);
        }
        if self.model.nic_read {
            let names: Vec<&str> = self.model.nics.iter().map(|n| n.name.as_str()).collect();
            s += &format!("nics: {} ({})\n", names.len(), names.join(" "));
        }
        if self.model.conn_read {
            let c = &self.model.conns;
            let scope = if self.net_listen { "listening" } else { "all" };
            let on = self.net_nic.as_deref().map(|n| format!(", on {n}")).unwrap_or_default();
            s += &format!("conns: {} of {} ({}, {scope}{on})\n", self.net_rows.len(), c.list.len(), c.source);
        }
        if self.model.disk_read {
            let d = &self.model.disks;
            let drives: Vec<&str> = d.drives.iter().map(|x| x.id.as_str()).collect();
            let vols: Vec<&str> = d.volumes.iter().map(|v| v.mount.as_str()).collect();
            s += &format!("disks: {} ({}); volumes: {} ({})\n", drives.len(), drives.join(" "), vols.len(), vols.join(" "));
        }
        if self.model.sensor_read {
            let m = &self.model.sensors;
            let (t, f) = (m.count(sensors::Kind::Temp), m.count(sensors::Kind::Fan) + m.count(sensors::Kind::Duty));
            s += &format!("sensors: {} (temps {t}, fans {f}; chips {})\n", m.list.len(), m.chips().len());
        }
        if let Some(h) = &self.model.hw {
            let note = h.note.map(|n| format!("; note {n:?}")).unwrap_or_default();
            s += &format!("hw: {} ({}); slots {} of {}{note}\n", crate::ui::sys::machine(self), h.source, h.used(), h.slots_total());
        }
        if let Some(st) = &self.model.st {
            s += &format!("topology: {} ({})\n", st.topo.summary(), st.topo.source);
        }
        let widths: Vec<String> = self.layouts[&self.screen]
            .layout(ito_layout::Rect::new(0.0, 0.0, 1000.0, 100.0), 0.0)
            .iter()
            .map(|(p, r)| format!("{}={:.0}", p.name(), r.w))
            .collect();
        s += &format!("widths: {}\n", widths.join(" "));
        match &self.confirm {
            Some(Ask::Kill(t)) => s += &format!("confirm: kill {} {}\n", t.pid, t.name),
            Some(Ask::Svc(t)) => s += &format!("confirm: {:?} {}\n", t.action, t.name),
            None => {}
        }
        if let Some((t, _)) = &self.toast {
            s += &format!("toast: {t}\n");
        }
        s
    }
}

/// The cursors of a model not looked at yet: Sys opens on its slots, its
/// left column being a page to scroll.
fn new_views() -> HashMap<Screen, View> {
    HashMap::from([(Screen::Sys, View { focus: Pane::Mid.index(), ..View::default() })])
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
    fn the_overview_has_a_gauge_per_device_and_enter_opens_its_screen() {
        let (mut a, tx, _ctl) = app();
        assert_eq!(a.ovw_gauges(), [Gauge::Cpu, Gauge::Mem, Gauge::Swap]);
        let dev = |id: &str, kind| gpu::Device { id: id.into(), kind, ..gpu::Device::default() };
        tx.send(Msg::Gpu(vec![dev("a", gpu::Kind::Gpu), dev("b", gpu::Kind::Npu)])).unwrap();
        let drive = |id: &str| disk::Drive { id: id.into(), ..disk::Drive::default() };
        tx.send(Msg::Disks(disk::Disks { drives: vec![drive("sda"), drive("sdb")], ..disk::Disks::default() })).unwrap();
        a.pump();
        assert_eq!(a.ovw_gauges(), [Gauge::Cpu, Gauge::Mem, Gauge::Swap, Gauge::Gpu(0), Gauge::Gpu(1), Gauge::Disk(0), Gauge::Disk(1)]);
        keys(&mut a, "j j j j <Enter>");
        assert_eq!((a.screen, a.selected_gpu().map(|d| d.id.as_str())), (Screen::Gpu, Some("b")));
        keys(&mut a, "1 j j <Enter>");
        assert_eq!((a.screen, a.selected_drive().map(|d| d.id.as_str())), (Screen::Disk, Some("sdb")));
        keys(&mut a, "1 k k k k k k <Enter>");
        assert_eq!(a.screen, Screen::Cpu);
        keys(&mut a, "1 j j <Enter>");
        assert_eq!(a.screen, Screen::Mem);
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
            a.layouts[&Screen::Cpu].layout(ito_layout::Rect::new(0.0, 0.0, 300.0, 10.0), 0.0).iter().map(|(_, r)| r.w).collect();
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
    fn tab_cycles_every_screen_and_enter_opens_a_process() {
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
        assert_eq!(a.screen, Screen::Disk);
        keys(&mut a, "<Tab>");
        assert_eq!(a.screen, Screen::Sensors);
        keys(&mut a, "<Tab>");
        assert_eq!(a.screen, Screen::Sys);
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
        let me = Proc { user: Some("me".into()), ..proc(std::process::id(), "yagura", 0.0, 1) };
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
    fn the_settings_key_opens_the_screen_and_esc_closes_it() {
        let (mut a, _tx, _ctl) = app();
        // Ctrl+, (Cmd+, on macOS): whatever the keymap binds.
        let open = a.keymap().main.iter().find(|b| b.run == [Act::Settings]).map(|b| b.on[0]).unwrap();
        a.feed(open);
        assert_eq!(a.mode, Mode::Settings);
        // Only the settings layer is looked at: a screen key does nothing.
        keys(&mut a, "3 q");
        assert_eq!((a.mode, a.screen, a.quit), (Mode::Settings, Screen::Ovw, false));
        keys(&mut a, "<Esc>");
        assert_eq!(a.mode, Mode::Normal);
        a.feed(open);
        a.feed(open);
        assert_eq!(a.mode, Mode::Normal);
    }

    #[test]
    fn y_copies_the_sample_or_the_cards_address() {
        use crate::config::settings::Host;
        let (mut a, _tx, _ctl) = app();
        keys(&mut a, "y");
        assert!(a.copy.is_none());
        assert_eq!(a.toast.as_ref().map(|t| t.0.as_str()), Some(tr("toast.copy_none")));
        // No [[host]] yet: the sample to paste into config.toml.
        keys(&mut a, "F y");
        assert_eq!(a.copy.take(), Some(remote::host_sample()));
        let (_ftx, frx) = crossbeam_channel::unbounded();
        a.set_fleet(vec![Peer::new(&Host { name: "nas".into(), addr: "localhost:7314".into() })], (frx, Vec::new()));
        keys(&mut a, "y");
        assert!(a.copy.is_none());
        keys(&mut a, "l y");
        assert_eq!(a.copy.take().as_deref(), Some("localhost:7314"));
        assert_eq!(a.toast.as_ref().map(|t| t.0.clone()), Some(trf("toast.copied", &["localhost:7314"])));
        // A line clicked in the fleet's hint.
        a.apply_out(Out { copy: Some("yagura --agent".into()), ..Out::default() }, &egui::Context::default());
        assert_eq!(a.copy.as_deref(), Some("yagura --agent"));
    }

    #[test]
    fn f_shows_the_fleet_and_its_keys_stay_on_the_cards() {
        use crate::config::settings::Host;
        use crate::remote::{Status, proto::Hello};
        let (mut a, _tx, _ctl) = app();
        let (ftx, frx) = crossbeam_channel::unbounded();
        let host = |n: &str| Peer::new(&Host { name: n.into(), addr: format!("localhost:{}", proto::PORT) });
        a.set_fleet(vec![host("nas"), host("pi"), host("mac")], (frx, Vec::new()));
        keys(&mut a, "3 F");
        assert!(a.fleet && a.report().contains("screen: fleet"), "{}", a.report());
        // Two cards in a row: down goes to the card under.
        a.apply_out(Out { fleet_cols: Some(2), ..Out::default() }, &egui::Context::default());
        keys(&mut a, "j");
        assert_eq!(a.fleet_sel, 2);
        keys(&mut a, "l l l G");
        assert_eq!((a.fleet_sel, a.screen), (3, Screen::Mem));
        keys(&mut a, "k g g");
        assert_eq!(a.fleet_sel, 0);
        // The hosts' threads report through the channel.
        ftx.send((1, Event::Up(Hello { proto: proto::PROTO, interval: 1.0, ..Hello::default() }))).unwrap();
        a.pump();
        assert_eq!(a.peers[1].status(Instant::now()), Status::Ok);
        assert_eq!(a.peers[0].status(Instant::now()), Status::Connecting);
        // View only: the card says why x does nothing.
        keys(&mut a, "l l x");
        assert_eq!((a.fleet_sel, a.mode), (2, Mode::Normal));
        assert_eq!(a.toast.as_ref().map(|t| t.0.clone()), Some(trf("toast.fleet_view_only", &["pi", "kill"])));
        keys(&mut a, "h h x");
        assert_eq!(a.toast.as_ref().map(|t| t.0.as_str()), Some(tr("toast.fleet_local")));
        // Esc closes it, F opens it on the same card, a screen key closes it there.
        keys(&mut a, "<Esc> F");
        assert!(a.fleet);
        keys(&mut a, "2");
        assert!(!a.fleet && a.screen == Screen::Cpu);
        a.apply_out(Out { fleet_tab: true, ..Out::default() }, &egui::Context::default());
        assert!(a.fleet);
        a.apply_out(Out { fleet_click: Some((9, false)), ..Out::default() }, &egui::Context::default());
        assert_eq!(a.fleet_sel, 3);
        // Enter on this machine's card is the screens behind.
        keys(&mut a, "g g <Enter>");
        assert!(!a.fleet && a.screen == Screen::Cpu);
    }

    #[test]
    fn enter_on_a_card_shows_that_machines_screens() {
        use crate::config::settings::Host;
        use crate::remote::{client::Why, proto::Hello};
        let (mut a, tx, ctl) = app();
        let (ftx, frx) = crossbeam_channel::unbounded();
        let (w0, wants0) = crossbeam_channel::unbounded();
        let (w1, wants1) = crossbeam_channel::unbounded();
        let host = |n: &str| Peer::new(&Host { name: n.into(), addr: format!("localhost:{}", proto::PORT) });
        a.set_fleet(vec![host("nas"), host("pi")], (frx, vec![w0, w1]));
        let hello = |user: &str| Hello { proto: proto::PROTO, interval: 2.0, user: user.into(), ..Hello::default() };
        tx.send(Msg::Procs { procs: vec![proc(7, "here", 1.0, 1)], total_threads: None }).unwrap();
        a.pump();
        // A machine that is not connected stays a card.
        keys(&mut a, "5 F l <Enter>");
        assert!(a.fleet && a.away.is_none());
        assert_eq!(a.toast.as_ref().map(|t| t.0.clone()), Some(trf("toast.fleet_down", &["nas"])));
        assert!(wants0.try_recv().is_err());
        // Up, it opens on the same screen with its own readings only.
        ftx.send((0, Event::Up(hello("ana")))).unwrap();
        a.pump();
        keys(&mut a, "<Enter>");
        assert!(!a.fleet && a.screen == Screen::Procs);
        assert_eq!(a.away_peer().map(|p| p.name.as_str()), Some("nas"));
        assert_eq!((a.me.as_deref(), a.rows.len()), (Some("ana"), 0));
        assert_eq!(wants0.try_iter().collect::<Vec<_>>(), [ToHost::Want(Want { detail: true, ..Want::default() })]);
        assert!(wants1.try_recv().is_err());
        assert!(a.report().contains("machine: nas"), "{}", a.report());
        ftx.send((0, Event::Msg(Box::new(Msg::Procs { procs: vec![proc(9, "there", 5.0, 1)], total_threads: None })))).unwrap();
        // This machine's readings go to its card, the other host's to nothing.
        tx.send(Msg::Procs { procs: vec![proc(8, "here2", 1.0, 1)], total_threads: None }).unwrap();
        ftx.send((1, Event::Msg(Box::new(Msg::Procs { procs: vec![proc(10, "pi", 5.0, 1)], total_threads: None })))).unwrap();
        a.pump();
        assert_eq!(a.selected_proc().map(|p| p.pid), Some(9));
        assert_eq!(a.home().procs.iter().map(|p| p.pid).collect::<Vec<_>>(), [8]);
        // Its svc screen asks it for services, not this machine's sampler.
        let _ = ctl.try_iter().count();
        keys(&mut a, "6");
        assert_eq!(wants0.try_iter().collect::<Vec<_>>(), [ToHost::Want(Want { detail: true, services: true, conns: false })]);
        assert!(ctl.try_recv().is_err());
        // Its [agent] allow lists nothing, so it is view only; sampling stays with it.
        keys(&mut a, "x");
        assert_eq!(a.toast.as_ref().map(|t| t.0.clone()), Some(trf("toast.fleet_view_only", &["nas", "service"])));
        assert!(a.confirm.is_none() && wants0.try_recv().is_err());
        keys(&mut a, "P");
        assert_eq!(a.toast.as_ref().map(|t| t.0.clone()), Some(trf("toast.remote_sampling", &["nas"])));
        // Lost, the screens keep its last readings.
        ftx.send((0, Event::Down(Why::Closed))).unwrap();
        a.pump();
        assert!(a.away.as_ref().is_some_and(|w| w.lost));
        assert!(a.report().contains("machine: nas (lost)"), "{}", a.report());
        ftx.send((0, Event::Up(hello("ana")))).unwrap();
        a.pump();
        assert!(a.away.as_ref().is_some_and(|w| !w.lost));
        // ] goes round: pi is down, so back to this machine, where it was.
        keys(&mut a, "]");
        assert!(a.away.is_none() && a.screen == Screen::Svc);
        assert_eq!(wants0.try_iter().collect::<Vec<_>>(), [ToHost::Want(Want::default())]);
        assert!(matches!(ctl.try_iter().collect::<Vec<_>>()[..], [Ctl::Services(true)]));
        assert_eq!(a.model.procs.iter().map(|p| p.pid).collect::<Vec<_>>(), [8]);
        assert!(a.report().contains("machine: this"), "{}", a.report());
        ftx.send((1, Event::Up(hello("")))).unwrap();
        a.pump();
        keys(&mut a, "[");
        assert_eq!(a.away_peer().map(|p| p.name.as_str()), Some("pi"));
        assert_eq!(a.me, None);
        keys(&mut a, "[");
        assert_eq!(a.away_peer().map(|p| p.name.as_str()), Some("nas"));
        // The machine inside stays a stop while it is down; once out of it,
        // with every host down, there is nowhere to go.
        keys(&mut a, "]");
        ftx.send((0, Event::Down(Why::Closed))).unwrap();
        ftx.send((1, Event::Down(Why::Closed))).unwrap();
        a.pump();
        keys(&mut a, "]");
        assert!(a.away.is_none());
        assert_eq!(a.toast.as_ref().map(|t| t.0.as_str()), Some(tr("toast.machine_home")));
        keys(&mut a, "]");
        assert_eq!(a.toast.as_ref().map(|t| t.0.as_str()), Some(tr("toast.machine_none")));
    }

    #[test]
    fn a_machine_that_allows_kill_is_asked_to_end_and_answers() {
        use crate::config::settings::Host;
        use crate::remote::{client::Why, proto::Hello};
        let (mut a, _tx, _ctl) = app();
        let (ftx, frx) = crossbeam_channel::unbounded();
        let (w0, wants0) = crossbeam_channel::unbounded();
        a.set_fleet(vec![Peer::new(&Host { name: "nas".into(), addr: format!("localhost:{}", proto::PORT) })], (frx, vec![w0]));
        let allow = vec!["kill".to_string()];
        let hello = Hello { proto: proto::PROTO, user: "ana".into(), os: "Linux 24.04 Ubuntu".into(), allow, pid: 3, ..Hello::default() };
        ftx.send((0, Event::Up(hello))).unwrap();
        a.pump();
        keys(&mut a, "5 F l <Enter>");
        assert_eq!(a.away_peer().map(|p| p.name.as_str()), Some("nas"));
        let procs = vec![proc(3, "yagura", 50.0, 1), proc(std::process::id(), "here", 1.0, 1), proc(7, "big", 80.0, 9)];
        ftx.send((0, Event::Msg(Box::new(Msg::Procs { procs, total_threads: None })))).unwrap();
        a.pump();
        let _ = wants0.try_iter().count();
        // Its agent is the one not to end; this machine's PID there is just a number.
        keys(&mut a, "l j x");
        assert_eq!((a.mode, a.toast.as_ref().map(|t| t.0.clone())), (Mode::Normal, Some(trf("toast.kill_agent", &["nas"]))));
        keys(&mut a, "k x");
        assert!(matches!(&a.confirm, Some(Ask::Kill(t)) if t.pid == 7), "{:?}", a.confirm);
        assert_eq!(a.family(), Family::Other);
        keys(&mut a, "y");
        assert_eq!(wants0.try_iter().collect::<Vec<_>>(), [ToHost::Act(1, proto::Act::Kill { pid: 7, start: 0 })]);
        let t = Target { pid: 7, name: "big".into(), start: 0 };
        assert_eq!(a.toast.as_ref().map(|t| t.0.clone()), Some(trf("toast.on_machine", &["nas", &Ask::Kill(t.clone()).starting()])));
        assert_eq!(a.asked.len(), 1);
        // The answer names the machine; one for another request is not this one's.
        ftx.send((0, Event::Done(9, Done::Kill(Outcome::Gone)))).unwrap();
        ftx.send((0, Event::Done(1, Done::Kill(Outcome::Ended)))).unwrap();
        a.pump();
        assert_eq!(a.toast.as_ref().map(|t| t.0.clone()), Some(trf("toast.on_machine", &["nas", &kill_toast(&t, Outcome::Ended)])));
        assert!(a.asked.is_empty());
        // A refusal says why, in its own words.
        keys(&mut a, "x y");
        ftx.send((0, Event::Done(2, Done::Refused(Refusal::NotAllowed)))).unwrap();
        a.pump();
        assert_eq!(a.toast.as_ref().map(|t| t.0.clone()), Some(trf("toast.fleet_view_only", &["nas", "kill"])));
        // Lost before it answers: said once, and the request is forgotten.
        keys(&mut a, "x y");
        assert_eq!(a.asked.len(), 1);
        ftx.send((0, Event::Down(Why::Closed))).unwrap();
        a.pump();
        assert_eq!(a.toast.as_ref().map(|t| t.0.clone()), Some(trf("toast.remote_no_answer", &["nas"])));
        assert!(a.asked.is_empty());
        // Down, nothing is sent.
        assert_eq!(wants0.try_iter().filter(|w| matches!(w, ToHost::Act(..))).count(), 2);
        keys(&mut a, "x y");
        assert_eq!(a.toast.as_ref().map(|t| t.0.clone()), Some(trf("toast.fleet_down", &["nas"])));
        assert!(a.asked.is_empty() && !wants0.try_iter().any(|w| matches!(w, ToHost::Act(..))));
        // service is not in its allow.
        keys(&mut a, "6 x");
        assert_eq!(a.toast.as_ref().map(|t| t.0.clone()), Some(trf("toast.fleet_view_only", &["nas", "service"])));
    }

    #[test]
    fn the_os_in_a_hello_picks_the_words() {
        assert_eq!(Family::of("Windows 11 Pro"), Family::Windows);
        assert_eq!(Family::of("macOS 15 Sequoia"), Family::Mac);
        assert_eq!(Family::of("Darwin 24.1"), Family::Mac);
        assert_eq!(Family::of("Linux 24.04 Ubuntu"), Family::Other);
        assert_eq!(Family::of(""), Family::Other);
    }

    #[test]
    fn x_asks_first_and_n_keeps_the_process() {
        let (mut a, tx, _ctl) = app();
        keys(&mut a, "x");
        assert_eq!((a.mode, a.toast.as_ref().map(|t| t.0.as_str())), (Mode::Normal, Some(tr("toast.kill_none"))));
        tx.send(Msg::Procs { procs: vec![proc(7, "big", 80.0, 9), proc(std::process::id(), "yagura", 1.0, 1)], total_threads: None })
            .unwrap();
        a.pump();
        keys(&mut a, "5 l x");
        assert_eq!(a.mode, Mode::Confirm);
        assert!(matches!(&a.confirm, Some(Ask::Kill(t)) if t.pid == 7), "{:?}", a.confirm);
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
    fn on_svc_s_x_and_r_ask_about_the_service_and_n_leaves_it() {
        let (mut a, tx, _ctl) = app();
        keys(&mut a, "6 S");
        assert_eq!(a.toast.as_ref().map(|t| t.0.as_str()), Some(tr("toast.svc_none")));
        let svc = |name: &str, state| svc::Service { name: name.into(), state, ..svc::Service::default() };
        let list = vec![svc("cron", svc::State::Running), svc("dead", svc::State::Failed)];
        tx.send(Msg::Svc(svc::Services { list, source: "systemd", note: None })).unwrap();
        a.pump();
        // Already where it would go: a toast, no question.
        keys(&mut a, "S");
        assert_eq!((a.mode, a.toast.as_ref().map(|t| t.0.clone())), (Mode::Normal, Some(trf("toast.svc_already_running", &["cron"]))));
        keys(&mut a, "x");
        assert_eq!(a.mode, Mode::Confirm);
        assert_eq!(a.confirm, Some(Ask::Svc(svcctl::Target { action: Action::Stop, name: "cron".into(), plist: None, driver: false })));
        keys(&mut a, "<Esc>");
        assert_eq!((a.mode, a.confirm.clone()), (Mode::Normal, None));
        keys(&mut a, "l j x");
        assert_eq!((a.mode, a.toast.as_ref().map(|t| t.0.clone())), (Mode::Normal, Some(trf("toast.svc_not_running", &["dead"]))));
        keys(&mut a, "R");
        assert_eq!(a.confirm, Some(Ask::Svc(svcctl::Target { action: Action::Restart, name: "dead".into(), plist: None, driver: false })));
        keys(&mut a, "n S");
        assert_eq!(a.confirm, Some(Ask::Svc(svcctl::Target { action: Action::Start, name: "dead".into(), plist: None, driver: false })));
        keys(&mut a, "q 5 S");
        assert_eq!((a.mode, a.toast.as_ref().map(|t| t.0.as_str())), (Mode::Normal, Some(tr("toast.svc_elsewhere"))));
    }

    #[test]
    fn a_job_not_loaded_is_started_from_its_plist_unless_it_is_disabled() {
        let (mut a, tx, _ctl) = app();
        let job = |name: &str, start| svc::Service {
            name: name.into(),
            state: svc::State::Stopped,
            start,
            file: Some(format!("/Library/LaunchAgents/{name}.plist")),
            unloaded: true,
            ..svc::Service::default()
        };
        let list = vec![job("off", svc::Start::Disabled), job("on", svc::Start::Manual)];
        tx.send(Msg::Svc(svc::Services { list, source: "launchctl", note: None })).unwrap();
        keys(&mut a, "6");
        a.pump();
        keys(&mut a, "l S");
        let hint = trf("toast.svc_disabled", &["off", &svcctl::enable_line("off")]);
        assert_eq!((a.mode, a.toast.as_ref().map(|t| t.0.clone())), (Mode::Normal, Some(hint)));
        keys(&mut a, "j R");
        let plist = Some("/Library/LaunchAgents/on.plist".to_string());
        assert_eq!(a.confirm, Some(Ask::Svc(svcctl::Target { action: Action::Restart, name: "on".into(), plist, driver: false })));
    }

    #[test]
    fn u_on_svc_switches_between_services_and_drivers() {
        let (mut a, tx, _ctl) = app();
        let svc = |name: &str, driver| svc::Service { name: name.into(), state: svc::State::Running, driver, ..svc::Service::default() };
        tx.send(Msg::Svc(svc::Services { list: vec![svc("cron", false)], source: "systemd", note: None })).unwrap();
        keys(&mut a, "6");
        a.pump();
        // No driver in the list (not Windows): a toast, the kind stays.
        keys(&mut a, "u");
        assert_eq!((a.svc_drivers, a.toast.as_ref().map(|t| t.0.as_str())), (false, Some(tr("toast.svc_no_drivers"))));
        let list = vec![svc("Spooler", false), svc("Tcpip", true), svc("disk", true)];
        tx.send(Msg::Svc(svc::Services { list, source: "SCM", note: None })).unwrap();
        a.pump();
        assert_eq!((a.svc_rows.len(), a.count_svc(None)), (1, 1));
        keys(&mut a, "u");
        assert_eq!((a.svc_drivers, a.svc_rows.len(), a.count_svc(None)), (true, 2, 2));
        assert_eq!(a.toast.as_ref().map(|t| t.0.clone()), Some(trf("toast.scope", &[tr("svc.drivers")])));
        keys(&mut a, "l x");
        assert_eq!(a.confirm, Some(Ask::Svc(svcctl::Target { action: Action::Stop, name: "Tcpip".into(), plist: None, driver: true })));
        keys(&mut a, "<Esc> u");
        assert_eq!((a.svc_drivers, a.selected_svc().map(|s| s.name.as_str())), (false, Some("Spooler")));
    }

    #[test]
    fn y_on_a_service_runs_on_a_thread_and_the_list_is_read_again() {
        let (mut a, tx, ctl) = app();
        // A name no OS has: the worker comes back refused or failed.
        let name = "yagura-test-no-such-service";
        let list = vec![svc::Service { name: name.into(), state: svc::State::Running, ..svc::Service::default() }];
        tx.send(Msg::Svc(svc::Services { list, source: "test", note: None })).unwrap();
        keys(&mut a, "6");
        a.pump();
        keys(&mut a, "x y");
        assert_eq!((a.mode, a.confirm.clone()), (Mode::Normal, None));
        let stopping = trf("toast.svc_stopping", &[name]);
        assert_eq!(a.toast.as_ref().map(|t| t.0.clone()), Some(stopping.clone()));
        let _: Vec<Ctl> = ctl.try_iter().collect();
        let until = Instant::now() + Duration::from_secs(20);
        while a.toast.as_ref().is_some_and(|t| t.0 == stopping) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
            a.pump();
        }
        let toast = a.toast.map(|t| t.0).unwrap_or_default();
        assert!(toast != stopping && toast.contains(name), "{toast}");
        assert!(matches!(ctl.try_iter().last(), Some(Ctl::Services(true))));
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
            conn(net::Proto::Tcp, "127.0.0.1:40000", Some("127.0.0.1:631"), net::State::Established, Some(7)),
        ];
        tx.send(Msg::Conns(net::Conns { list, source: "/proc/net", note: None })).unwrap();
        let nic = |name: &str, kind, ip: &str| net::Nic { name: name.into(), kind, up: true, ipv4: vec![ip.into()], ..net::Nic::default() };
        tx.send(Msg::Nics(vec![nic("eth0", net::Kind::Wired, "10.0.0.2/24"), nic("lo", net::Kind::Loopback, "127.0.0.1/8")])).unwrap();
        a.pump();
        keys(&mut a, "7");
        assert_eq!((a.net_rows.len(), a.selected_nic().map(|n| n.name.as_str())), (4, Some("eth0")));
        // u keeps the sockets that wait for others: TCP listening and unconnected UDP.
        keys(&mut a, "u");
        assert!(a.net_listen);
        assert_eq!(a.net_rows.len(), 2);
        assert_eq!(a.toast.as_ref().map(|t| t.0.clone()), Some(trf("toast.scope", &[tr("net.listening")])));
        keys(&mut a, "u");
        assert_eq!(a.net_rows.len(), 4);
        // Enter on an interface keeps its sockets (and those on any address)
        // and moves to them; one with no owner toasts.
        keys(&mut a, "<Enter>");
        assert_eq!((a.focus(), a.net_nic.as_deref(), a.net_rows.len()), (Pane::Mid, Some("eth0"), 3));
        assert_eq!(a.toast.as_ref().map(|t| t.0.clone()), Some(trf("toast.net_nic", &["eth0"])));
        keys(&mut a, "j <Enter>");
        assert_eq!(a.toast.as_ref().map(|t| t.0.clone()), Some(trf("toast.net_no_pid", &["10.0.0.2:50000"])));
        // The narrowing follows the selected interface, and Esc drops it.
        keys(&mut a, "h j");
        assert_eq!(a.net_nic.as_deref(), Some("lo"));
        assert!(a.net_rows.iter().all(|&i| a.model.conns.list[i].local.port() != 50000));
        assert_eq!(a.net_rows.len(), 3);
        keys(&mut a, "<Esc>");
        assert_eq!((a.net_nic.as_deref(), a.net_rows.len()), (None, 4));
        // Moving between interfaces no longer narrows.
        keys(&mut a, "k");
        assert_eq!((a.net_nic.as_deref(), a.net_rows.len()), (None, 4));
        // The filter matches the process name, and the selection follows the socket.
        keys(&mut a, "l / s s h <Enter>");
        assert_eq!((a.screen, a.net_rows.len()), (Screen::Net, 2));
        keys(&mut a, "g g");
        assert_eq!(a.selected_conn().map(|c| c.local.port()), Some(22));
        keys(&mut a, "<Enter>");
        assert_eq!(a.screen, Screen::Procs);
        assert_eq!(a.selected_proc().map(|p| p.pid), Some(7));
        assert!(matches!(ctl.try_iter().last(), Some(Ctl::Net(false))));
    }

    #[test]
    fn enter_on_the_disk_screen_moves_between_a_drive_and_its_volumes() {
        let (mut a, tx, _ctl) = app();
        let drive = |id: &str| disk::Drive { id: id.into(), ..disk::Drive::default() };
        let vol = |m: &str, d: Option<&str>| disk::Volume { mount: m.into(), drive: d.map(Into::into), ..disk::Volume::default() };
        let volumes = vec![vol("/", Some("sda")), vol("/boot", Some("sdb")), vol("/home", Some("sdb")), vol("/net", None)];
        tx.send(Msg::Disks(disk::Disks { drives: vec![drive("sda"), drive("sdb")], volumes, note: None })).unwrap();
        a.pump();
        keys(&mut a, "8 j <Enter>");
        assert_eq!((a.focus(), a.selected_volume().map(|v| v.mount.as_str())), (Pane::Mid, Some("/boot")));
        keys(&mut a, "k <Enter>");
        assert_eq!((a.focus(), a.selected_drive().map(|d| d.id.as_str())), (Pane::Left, Some("sda")));
        // A volume on no known drive stays where it is.
        keys(&mut a, "l G <Enter>");
        assert_eq!((a.focus(), a.selected_volume().map(|v| v.mount.as_str())), (Pane::Mid, Some("/net")));
        assert!(a.report().contains("disks: 2 (sda sdb); volumes: 4"));
    }

    #[test]
    fn enter_on_the_sensors_screen_moves_between_a_chip_and_its_sensors() {
        let (mut a, tx, _ctl) = app();
        let temp = |chip: &str, label: &str| sensors::Sensor {
            id: format!("{chip}/{label}"),
            chip_id: chip.into(),
            chip: chip.into(),
            label: label.into(),
            ..sensors::Sensor::default()
        };
        let list = vec![temp("coretemp", "core0"), temp("coretemp", "core1"), temp("nvme", "temp1"), temp("nvme", "temp2")];
        tx.send(Msg::Sensors(sensors::Sensors { list, note: None })).unwrap();
        a.pump();
        keys(&mut a, "9 j <Enter>");
        assert_eq!((a.focus(), a.selected_sensor().map(|s| s.id.as_str())), (Pane::Mid, Some("nvme/temp1")));
        keys(&mut a, "j k k <Enter>");
        assert_eq!((a.focus(), a.selected_chip().map(|c| c.name)), (Pane::Left, Some("coretemp")));
        assert!(a.report().contains("sensors: 4 (temps 4, fans 0; chips 2)"));
    }

    #[test]
    fn sys_opens_on_its_slots_and_the_left_column_skips_blank_facts() {
        let (mut a, tx, _ctl) = app();
        keys(&mut a, "0");
        assert_eq!((a.screen, a.focus(), a.count(Screen::Sys, Pane::Mid)), (Screen::Sys, Pane::Mid, 0));
        let slot = |locator: &str, size| hw::Slot {
            locator: locator.into(),
            present: size > 0,
            size: (size > 0).then_some(size),
            ..hw::Slot::default()
        };
        let cpu =
            hw::Cpu { socket: "AM5".into(), name: "AMD Ryzen 7 7700".into(), cores: 8, threads: 16, max_mhz: 5300, ..hw::Cpu::default() };
        let h = hw::Hardware {
            system: hw::Part { vendor: "ASUS".into(), name: "ROG STRIX B650E-F".into(), ..hw::Part::default() },
            cpus: vec![cpu.clone(), cpu],
            slots: vec![slot("DIMM_A1", 0), slot("DIMM_A2", 16 << 30), slot("DIMM_B1", 0), slot("DIMM_B2", 16 << 30)],
            source: "SMBIOS 3.5 (test)".into(),
            ..hw::Hardware::default()
        };
        tx.send(Msg::Hardware(Box::new(h))).unwrap();
        a.pump();
        keys(&mut a, "j");
        assert_eq!(a.selected_slot().map(|s| s.locator.as_str()), Some("DIMM_A2"));
        use crate::ui::sys::{Line, lines};
        let l = lines(&a);
        // No board facts: no such section, and no blank rows.
        assert!(!l.contains(&Line::Head(tr("sys.board"))));
        assert!(!l.contains(&Line::Kv(tr("sys.version"), String::new())));
        assert!(l.contains(&Line::Kv(tr("sys.slots"), trf("sys.slots_used", &["2", "4"]))));
        assert!(l.contains(&Line::Kv(tr("sys.installed"), "32.0 GiB".into())));
        // Two sockets add up; the caches the firmware leaves out are left out.
        let at = l.iter().position(|x| *x == Line::Head(tr("sys.processor"))).unwrap();
        assert_eq!(
            l[at + 1..at + 6],
            [
                Line::Kv(tr("sys.model"), "AMD Ryzen 7 7700".into()),
                Line::Kv(tr("sys.socket"), trf("sys.sockets", &["2", "AM5"])),
                Line::Kv(tr("sys.cores"), "16".into()),
                Line::Kv(tr("sys.threads"), "32".into()),
                Line::Kv(tr("sys.max_speed"), trf("sys.mhz", &["5300"])),
            ]
        );
        assert!(!l.iter().any(|x| matches!(x, Line::Kv(k, _) if *k == tr("sys.l1"))));
        // No GPUs or drives read yet: no such sections.
        assert!(!l.contains(&Line::Head(tr("sys.graphics"))));
        assert!(!l.contains(&Line::Head(tr("sys.drives"))));
        let gpu = gpu::Device { name: "Radeon RX 7800 XT".into(), vram_total: Some(16 << 30), ..gpu::Device::default() };
        let npu = gpu::Device { kind: gpu::Kind::Npu, vendor: Some("Intel"), ..gpu::Device::default() };
        tx.send(Msg::Gpu(vec![gpu, npu])).unwrap();
        let drive = disk::Drive { name: "nvme0n1".into(), size: Some(2_000_398_934_016), kind: disk::Kind::Ssd, ..disk::Drive::default() };
        tx.send(Msg::Disks(disk::Disks { drives: vec![drive], ..disk::Disks::default() })).unwrap();
        a.pump();
        let l = lines(&a);
        let at = l.iter().position(|x| *x == Line::Head(tr("sys.graphics"))).unwrap();
        assert_eq!(
            l[at + 1..at + 7],
            [
                Line::Kv(tr("gpu.kind_gpu"), "Radeon RX 7800 XT · 16.0 GiB".into()),
                Line::Kv(tr("gpu.kind_npu"), trf("gpu.unnamed", &["Intel", tr("gpu.kind_npu")])),
                Line::Kv(tr("sys.details"), trf("sys.press", &["4"])),
                Line::Head(tr("sys.drives")),
                Line::Kv(tr("disk.kind.ssd"), "nvme0n1 · 1.8 TiB".into()),
                Line::Kv(tr("sys.details"), trf("sys.press", &["8"])),
            ]
        );
        assert_eq!(a.count(Screen::Sys, Pane::Left), l.len());
        assert!(a.report().contains("hw: ASUS ROG STRIX B650E-F (SMBIOS 3.5 (test)); slots 2 of 4\n"), "{}", a.report());
        // A self-built PC: the system is left blank and the board names it.
        let h = hw::Hardware {
            system: hw::Part { vendor: "ASUS".into(), version: "1.0".into(), ..hw::Part::default() },
            board: hw::Part { vendor: "ASUSTeK COMPUTER INC.".into(), name: "PRIME B650M-A".into(), ..hw::Part::default() },
            ..hw::Hardware::default()
        };
        tx.send(Msg::Hardware(Box::new(h))).unwrap();
        a.pump();
        assert_eq!(crate::ui::sys::machine(&a), "ASUSTeK COMPUTER INC. PRIME B650M-A");
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
