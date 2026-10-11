//! `yagura --agent`: no window, the same sampler, and a summary for every
//! viewer connected to 127.0.0.1, or directly to `[agent] listen` with a key
//! the agent knows ([`super::noise`]).
//!
//! One hub thread folds the sampler's messages into a [`Model`] and hands
//! each viewer the frames through its own short channel; a viewer that falls
//! behind misses summaries rather than holding the others up. A viewer that
//! watches the machine (`ToAgent::Watch`) also gets the sampler's readings,
//! starting with the latest of each; processes, services and sockets are
//! read only while somebody wants them. After the first summary each viewer
//! gets only what changed ([`proto::Delta`]), as often as it asked
//! ([`ToAgent::Pace`]).
//!
//! A viewer may also ask the agent to end a process or start, stop or
//! restart a service ([`ToAgent::Act`]). The agent decides from its own
//! `[agent] allow` and its own readings ([`check`]), runs each request on a
//! thread of its own, logs it to stderr (and to `--log`) and answers with
//! [`FromAgent::Done`].

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::mem::Discriminant;
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded, select, unbounded};

use super::noise::{self, Keys};
use super::proto::{self, Act, Done, FromAgent, Hello, PROTO, Refusal, Summary, ToAgent, Want, Warn};
use super::wire::{self, Route};
use crate::collect::svc::State;
use crate::collect::{self, Ctl, Msg, gpu};
use crate::config::settings::ViewerKey;
use crate::model::Model;
use crate::{kill, svcctl};

/// A frame shared by every viewer.
type Frame = Arc<Vec<u8>>;

/// Frames a viewer may have waiting before it misses some.
const QUEUE: usize = 64;
/// A viewer pings every few seconds; one silent this long is gone.
const QUIET: Duration = Duration::from_secs(30);
/// How long an answer may wait behind the viewer's other frames.
const ANSWER: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub struct Config {
    /// `[agent] allow`.
    pub allow: Vec<String>,
    /// Seconds between samples.
    pub interval: f32,
    /// `[agent] listen`, when it is set.
    pub direct: Option<Direct>,
}

/// Where and to whom the agent answers directly.
#[derive(Clone, Debug)]
pub struct Direct {
    /// `address:port`.
    pub listen: String,
    /// This machine's.
    pub keys: Keys,
    /// `[[agent.viewer]]`.
    pub viewers: Vec<ViewerKey>,
}

/// The `--log` file, when given.
static LOG: OnceLock<Mutex<File>> = OnceLock::new();

/// A log larger than this is started over when the agent starts.
const LOG_MAX: u64 = 1 << 20;

/// Also write what the agent says to `path` (Windows' release build has no
/// console, so an agent started at logon would say it to nobody).
pub fn log_to(path: &Path) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let big = std::fs::metadata(path).is_ok_and(|m| m.len() > LOG_MAX);
    let f = OpenOptions::new().create(true).append(!big).write(true).truncate(big).open(path)?;
    let _ = LOG.set(Mutex::new(f));
    Ok(())
}

/// A line to stderr and the log.
fn say(line: std::fmt::Arguments) {
    eprintln!("{line}");
    if let Some(f) = LOG.get()
        && let Ok(mut f) = f.lock()
    {
        let _ = writeln!(f, "{} {line}", chrono::Local::now().format("%F %T"));
    }
}

/// Listen on `127.0.0.1:port` until killed.
pub fn run(port: u16, cfg: Config) -> io::Result<()> {
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            say(format_args!("yagura agent: cannot listen on 127.0.0.1:{port}: {e}"));
            return Err(e);
        }
    };
    let what = if cfg.allow.is_empty() { "view only".to_string() } else { format!("allows {}", cfg.allow.join(", ")) };
    say(format_args!("yagura {} agent: listening on {} ({what})", env!("CARGO_PKG_VERSION"), listener.local_addr()?));
    say(format_args!(
        "yagura agent: reach it with `ssh -L {port}:localhost:{port} <this host>` or `tailscale serve --bg --tcp {port} tcp://localhost:{port}`"
    ));
    let direct = match &cfg.direct {
        None => None,
        Some(d) => match TcpListener::bind(&d.listen) {
            Ok(l) => {
                say(format_args!(
                    "yagura agent: viewers with a key connect directly to {} (key {})",
                    l.local_addr()?,
                    noise::encode(&d.keys.public)
                ));
                if d.viewers.is_empty() {
                    say(format_args!(
                        "yagura agent: no [[agent.viewer]] keys yet, so nobody gets in directly; `yagura --key` on the viewer prints its own"
                    ));
                }
                Some(l)
            }
            Err(e) => {
                say(format_args!("yagura agent: cannot listen on {}: {e}", d.listen));
                return Err(e);
            }
        },
    };
    serve_all(listener, direct, cfg)
}

/// Serve plain viewers on `listener` (the tests pass one on port 0).
#[cfg(test)]
pub fn serve(listener: TcpListener, cfg: Config) -> io::Result<()> {
    serve_all(listener, None, cfg)
}

/// Serve plain viewers on `plain` and, with `cfg.direct`, viewers with a key
/// on `direct`.
pub fn serve_all(plain: TcpListener, direct: Option<TcpListener>, cfg: Config) -> io::Result<()> {
    let port = plain.local_addr()?.port();
    let (hub_tx, hub_rx) = unbounded();
    let direct = direct.zip(cfg.direct.clone());
    let at = direct.as_ref().and_then(|(l, _)| l.local_addr().ok());
    let hub_cfg = cfg.clone();
    std::thread::Builder::new().name("yagura-agent".into()).spawn(move || hub(&hub_cfg, port, at, &hub_rx))?;
    if let Some((l, d)) = direct {
        let hub = hub_tx.clone();
        std::thread::Builder::new().name("yagura-direct".into()).spawn(move || accept(&l, &hub, Some(&Arc::new(d))))?;
    }
    accept(&plain, &hub_tx, None);
    Ok(())
}

/// A thread for each viewer that connects to `l`.
fn accept(l: &TcpListener, hub: &Sender<HubIn>, direct: Option<&Arc<Direct>>) {
    for conn in l.incoming() {
        match conn {
            Ok(s) => {
                let (hub, direct) = (hub.clone(), direct.cloned());
                let spawned = std::thread::Builder::new().name("yagura-viewer".into()).spawn(move || viewer(s, &hub, direct.as_deref()));
                if let Err(e) = spawned {
                    say(format_args!("yagura agent: {e}"));
                }
            }
            Err(e) => say(format_args!("yagura agent: {e}")),
        }
    }
}

/// What a viewer thread tells the hub.
enum HubIn {
    Join(u64, Sender<Frame>),
    Want(u64, Want),
    /// Seconds between this viewer's summaries.
    Pace(u64, f32),
    Leave(u64),
    /// A viewer at `who` asks for `act`; the answer goes on `tx`.
    Act {
        id: u64,
        who: String,
        act: Act,
        tx: Sender<Frame>,
    },
}

struct Viewer {
    id: u64,
    tx: Sender<Frame>,
    want: Want,
    /// Between its summaries, and when the next is due.
    every: Duration,
    next: Instant,
    /// The summary it last got, which the next delta is made against;
    /// `None` when it missed one, so it gets the whole of the next.
    base: Option<Summary>,
}

impl Viewer {
    /// `s` if one is due by the round after this (`half` a round early is
    /// on time); false when it is gone.
    fn offer(&mut self, s: &Summary, now: Instant, half: Duration) -> bool {
        if now + half < self.next {
            return true;
        }
        let msg = match &self.base {
            Some(b) => FromAgent::Delta(b.diff(s)),
            None => FromAgent::Summary(s.clone()),
        };
        match self.tx.try_send(Arc::new(proto::frame(&msg))) {
            Ok(()) => {
                self.base = Some(s.clone());
                self.next = now + self.every;
            }
            Err(TrySendError::Full(_)) => self.base = None,
            Err(TrySendError::Disconnected(_)) => return false,
        }
        true
    }
}

/// A viewer's pace as the agent keeps it: within the limits and no faster
/// than the agent samples; `None` for nonsense.
fn pace(secs: f32, interval: Duration) -> Option<Duration> {
    secs.is_finite().then(|| Duration::from_secs_f32(secs.clamp(proto::PACE_MIN, proto::PACE_MAX)).max(interval))
}

/// The latest frame of each reading, for a viewer that starts watching.
#[derive(Default)]
struct Cache {
    /// `Static` and `Hardware`.
    fixed: Vec<(Discriminant<Msg>, Frame)>,
    detail: Vec<(Discriminant<Msg>, Frame)>,
    svc: Option<Frame>,
    conns: Option<Frame>,
}

impl Cache {
    fn keep(&mut self, r: Route, kind: Discriminant<Msg>, f: Frame) {
        let put = |list: &mut Vec<(Discriminant<Msg>, Frame)>| match list.iter_mut().find(|(k, _)| *k == kind) {
            Some(slot) => slot.1 = f.clone(),
            None => list.push((kind, f.clone())),
        };
        match r {
            Route::Static => put(&mut self.fixed),
            Route::Detail => put(&mut self.detail),
            Route::Services => self.svc = Some(f),
            Route::Conns => self.conns = Some(f),
        }
    }

    /// What a viewer that wanted `old` and now wants `new` has not had yet.
    fn news(&self, old: Want, new: Want) -> Vec<Frame> {
        let mut out = Vec::new();
        if new.detail && !old.detail {
            out.extend(self.fixed.iter().chain(&self.detail).map(|(_, f)| f.clone()));
        }
        if new.takes(Route::Services) && !old.takes(Route::Services) {
            out.extend(self.svc.clone());
        }
        if new.takes(Route::Conns) && !old.takes(Route::Conns) {
            out.extend(self.conns.clone());
        }
        out
    }
}

/// A reading as a frame for the viewers, with long process lines cut;
/// `None` when even so it is too large to send.
fn msg_frame(m: &Msg) -> Option<Frame> {
    let mut m = m.clone();
    if let Msg::Procs { procs, .. } = &mut m {
        wire::trim_procs(procs);
    }
    let f = proto::frame(&FromAgent::Msg(Box::new(m)));
    (f.len() <= 4 + proto::MAX_FRAME).then(|| Arc::new(f))
}

fn hub(cfg: &Config, port: u16, direct: Option<SocketAddr>, ins: &Receiver<HubIn>) {
    let interval = Duration::from_secs_f32(cfg.interval);
    // `ctl` stays alive here: the sampler stops when it is dropped.
    let (rx, ctl) = collect::spawn(None, interval);
    // Processes, services and sockets only while somebody watches.
    let _ = ctl.send(Ctl::Procs(0));
    let mut reading = Want::default();
    let procs_kind = std::mem::discriminant(&Msg::Procs { procs: Vec::new(), total_threads: None });
    let mut model = Model::default();
    let mut hello: Option<Frame> = None;
    let mut last: Option<Summary> = None;
    let mut cache = Cache::default();
    let mut viewers: Vec<Viewer> = Vec::new();
    // A service request that finished: read the services again at once.
    let (re_tx, re_rx) = unbounded::<()>();
    loop {
        select! {
            recv(rx) -> m => {
                let Ok(m) = m else { return };
                // The sensors come last in each round: the summary is whole then.
                let (first, round) = (matches!(m, Msg::Static(_)), matches!(m, Msg::Sensors(_)));
                let r = wire::route(&m);
                if let Some(f) = msg_frame(&m) {
                    cache.keep(r, std::mem::discriminant(&m), f.clone());
                    send(&mut viewers, &f, |w| w.takes(r));
                }
                model.apply(m);
                if first {
                    let f: Frame = Arc::new(proto::frame(&FromAgent::Hello(hello_of(&model, cfg, port, direct))));
                    hello = Some(f.clone());
                    send(&mut viewers, &f, |_| true);
                } else if round {
                    let s = Summary { viewers: viewers.len().min(usize::from(u16::MAX)) as u16, ..summary(&model) };
                    let now = Instant::now();
                    viewers.retain_mut(|v| v.offer(&s, now, interval / 2));
                    last = Some(s);
                } else {
                    continue;
                }
            }
            recv(ins) -> m => {
                match m {
                    Ok(HubIn::Join(id, tx)) => {
                        // One who comes before the hello gets it with everyone else.
                        if let Some(f) = &hello {
                            let _ = tx.try_send(f.clone());
                        }
                        let now = Instant::now();
                        let mut v = Viewer { id, tx, want: Want::default(), every: interval, next: now, base: None };
                        if let Some(s) = &last {
                            v.offer(s, now, Duration::ZERO);
                        }
                        viewers.push(v);
                    }
                    Ok(HubIn::Pace(id, secs)) => {
                        if let (Some(v), Some(every)) = (viewers.iter_mut().find(|v| v.id == id), pace(secs, interval)) {
                            v.every = every;
                            v.next = v.next.min(Instant::now() + every);
                        }
                    }
                    Ok(HubIn::Want(id, w)) => {
                        if let Some(v) = viewers.iter_mut().find(|v| v.id == id) {
                            for f in cache.news(v.want, w) {
                                let _ = v.tx.try_send(f);
                            }
                            v.want = w;
                        }
                    }
                    Ok(HubIn::Leave(id)) => viewers.retain(|v| v.id != id),
                    Ok(HubIn::Act { id, who, act, tx }) => {
                        let job = check(&cfg.allow, &act, &model, std::process::id());
                        do_job(id, who, act, job, tx, re_tx.clone());
                    }
                    Err(_) => return,
                }
            }
            recv(re_rx) -> _ => {
                if reading.services {
                    let _ = ctl.send(Ctl::Services(true));
                }
            }
        }
        let want = wire::union(viewers.iter().map(|v| v.want));
        if want != reading {
            if want.detail != reading.detail {
                let _ = ctl.send(Ctl::Procs(if want.detail { wire::procs_every(cfg.interval) } else { 0 }));
                if !want.detail {
                    cache.detail.retain(|(k, _)| *k != procs_kind);
                }
            }
            if want.services != reading.services {
                let _ = ctl.send(Ctl::Services(want.services));
                cache.svc = None;
            }
            if want.conns != reading.conns {
                let _ = ctl.send(Ctl::Net(want.conns));
                cache.conns = None;
            }
            reading = want;
        }
    }
}

/// What the agent will run for a request it let through.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Job {
    Kill(kill::Target),
    Svc(svcctl::Target),
}

/// Whether the agent does `act`, judged by its own `[agent] allow`, its own
/// readings and its own PID `me`; never by what the viewer says of them.
pub(crate) fn check(allow: &[String], act: &Act, model: &Model, me: u32) -> Result<Job, Refusal> {
    if !proto::allowed(allow, act.word()) {
        return Err(Refusal::NotAllowed);
    }
    match act {
        Act::Kill { pid, .. } if *pid == me => Err(Refusal::Agent),
        &Act::Kill { pid, start } => {
            let name = model.procs.iter().find(|p| p.pid == pid).map(|p| p.name.clone()).unwrap_or_default();
            Ok(Job::Kill(kill::Target { pid, name, start }))
        }
        Act::Service { action, name, driver } => {
            let s = model.services.list.iter().find(|s| s.name == *name && s.driver == *driver).ok_or(Refusal::NoService)?;
            // A launchd job known only from its plist: loaded from the plist
            // this machine read, unless it is disabled.
            let mut plist = None;
            if s.unloaded && *action != svcctl::Action::Stop {
                if s.start == crate::collect::svc::Start::Disabled {
                    return Err(Refusal::Disabled(svcctl::enable_line(name)));
                }
                plist = s.file.clone();
            }
            Ok(Job::Svc(svcctl::Target { action: *action, name: name.clone(), plist, driver: *driver }))
        }
    }
}

/// Run `job` (or report its refusal) on a thread of its own, log it and
/// answer the viewer.
fn do_job(id: u64, who: String, act: Act, job: Result<Job, Refusal>, tx: Sender<Frame>, reread: Sender<()>) {
    let run = move || {
        let what = match &act {
            Act::Kill { pid, start } => format!("end {pid} (started {start})"),
            Act::Service { action, name, .. } => format!("{action:?} {name}").to_lowercase(),
        };
        let outcome = match job {
            Err(r) => Done::Refused(r),
            Ok(Job::Kill(t)) => Done::Kill(kill::end(&t, kill::GRACE)),
            Ok(Job::Svc(t)) => {
                let o = svcctl::run(&t);
                let _ = reread.send(());
                Done::Svc(o)
            }
        };
        say(format_args!("yagura agent: {who} asks to {what}: {outcome:?}"));
        let _ = tx.send_timeout(Arc::new(proto::frame(&FromAgent::Done { id, outcome })), ANSWER);
    };
    if let Err(e) = std::thread::Builder::new().name("yagura-act".into()).spawn(run) {
        say(format_args!("yagura agent: {e}"));
    }
}

/// `f` to every viewer `to` picks; those gone are dropped.
fn send(viewers: &mut Vec<Viewer>, f: &Frame, to: impl Fn(Want) -> bool) {
    viewers.retain(|v| !to(v.want) || !matches!(v.tx.try_send(f.clone()), Err(TrySendError::Disconnected(_))));
}

fn hello_of(m: &Model, cfg: &Config, port: u16, direct: Option<SocketAddr>) -> Hello {
    let st = m.st.as_ref();
    let text = |f: fn(&collect::Static) -> &String| st.map(f).cloned().unwrap_or_default();
    Hello {
        proto: PROTO,
        version: env!("CARGO_PKG_VERSION").into(),
        host: text(|s| &s.host),
        os: text(|s| &s.os),
        arch: text(|s| &s.arch),
        brand: text(|s| &s.brand),
        boot_time: st.map_or(0, |s| s.boot_time),
        user: std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default(),
        allow: cfg.allow.clone(),
        interval: cfg.interval,
        pid: std::process::id(),
        port,
        direct: direct.map(|a| a.to_string()).unwrap_or_default(),
        key: cfg.direct.as_ref().filter(|_| direct.is_some()).map(|d| noise::encode(&d.keys.public)).unwrap_or_default(),
    }
}

/// The card's numbers from the latest readings.
pub fn summary(m: &Model) -> Summary {
    let gpu = m.gpus.iter().filter(|d| d.kind == gpu::Kind::Gpu).find_map(|d| {
        let name = if d.name.is_empty() { d.vendor.unwrap_or("GPU").to_string() } else { d.name.clone() };
        d.usage.map(|u| (name, u))
    });
    let n = |c: usize| c.min(usize::from(u16::MAX)) as u16;
    let warn = Warn {
        hot: n(m.sensors.hot()),
        drives: n(m.disks.drives.iter().filter(|d| d.warns()).count()),
        failed: m.svc_read.then(|| n(m.services.count(State::Failed))),
    };
    Summary {
        cpu: m.total,
        mem_used: m.mem.used,
        mem_total: m.mem.total,
        gpu,
        temp: m.sensors.hottest().map(|s| s.value),
        battery: m.battery,
        warn,
        viewers: 0,
    }
}

/// One viewer: the handshake when it connects directly, its hello, then
/// frames out and pings in until either side stops.
fn viewer(s: TcpStream, hub: &Sender<HubIn>, direct: Option<&Direct>) {
    let peer = s.peer_addr().map(|a| a.to_string()).unwrap_or_default();
    let _ = s.set_nodelay(true);
    let halves = match direct {
        None => noise::plain(&s).map(|(r, w)| (peer.clone(), r, w)),
        Some(d) => noise::respond(&s, &d.keys, |k| d.viewers.iter().find(|v| v.key == *k).map(|v| format!("{} ({peer})", v.name))),
    };
    match halves {
        Ok((who, r, w)) => {
            if let Err(e) = talk(&s, r, w, &who, hub) {
                say(format_args!("yagura agent: {who}: {e}"));
            }
        }
        Err(e) => say(format_args!("yagura agent: {peer}: {e}")),
    }
    let _ = s.shutdown(Shutdown::Both);
}

/// Tells the hub a viewer is gone however its thread ends.
struct Leave<'a>(&'a Sender<HubIn>, u64);

impl Drop for Leave<'_> {
    fn drop(&mut self) {
        let _ = self.0.send(HubIn::Leave(self.1));
    }
}

/// A viewer's frames over `r` and `w`, the halves of `s`.
fn talk(s: &TcpStream, mut r: impl Read, mut w: impl Write + Send + 'static, who: &str, hub: &Sender<HubIn>) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    s.set_read_timeout(Some(QUIET))?;
    let first = proto::read_body(&mut r)?;
    match proto::head(&first) {
        Some((0, p)) if p != PROTO => say(format_args!("yagura agent: a viewer speaks protocol {p}, this agent {PROTO}")),
        Some((0, _)) => {}
        _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "not a yagura viewer")),
    }
    let (tx, rx) = bounded::<Frame>(QUEUE);
    std::thread::Builder::new().name("yagura-viewer-out".into()).spawn(move || {
        for f in rx {
            if w.write_all(&f).and_then(|()| w.flush()).is_err() {
                break;
            }
        }
    })?;
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    if hub.send(HubIn::Join(id, tx.clone())).is_err() {
        return Ok(());
    }
    let _leave = Leave(hub, id);
    loop {
        match proto::read_frame::<ToAgent>(&mut r)? {
            ToAgent::Ping(n) => {
                if let Err(TrySendError::Disconnected(_)) = tx.try_send(Arc::new(proto::frame(&FromAgent::Pong(n)))) {
                    return Ok(());
                }
            }
            ToAgent::Watch(w) => {
                if hub.send(HubIn::Want(id, w)).is_err() {
                    return Ok(());
                }
            }
            ToAgent::Pace(secs) => {
                if hub.send(HubIn::Pace(id, secs)).is_err() {
                    return Ok(());
                }
            }
            ToAgent::Act { id, act } => {
                if hub.send(HubIn::Act { id, who: who.to_string(), act, tx: tx.clone() }).is_err() {
                    return Ok(());
                }
            }
            ToAgent::Hello { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect::Proc;
    use crate::collect::svc::{Service, Start};
    use crate::svcctl::Action;

    fn allow(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    fn model() -> Model {
        let mut m = Model { procs: vec![Proc { pid: 42, name: "sleep".into(), ..Proc::default() }], ..Model::default() };
        let svc = |name: &str, unloaded: bool, start: Start, driver: bool| Service {
            name: name.into(),
            start,
            unloaded,
            driver,
            file: unloaded.then(|| format!("/Library/LaunchDaemons/{name}.plist")),
            ..Service::default()
        };
        m.services.list = vec![
            svc("cron", false, Start::Auto, false),
            svc("job", true, Start::Manual, false),
            svc("off", true, Start::Disabled, false),
            svc("disk", false, Start::Auto, true),
        ];
        m
    }

    fn service(action: Action, name: &str, driver: bool) -> Act {
        Act::Service { action, name: name.into(), driver }
    }

    #[test]
    fn a_pace_is_kept_within_the_limits() {
        let second = Duration::from_secs(1);
        assert_eq!(pace(10.0, second), Some(Duration::from_secs(10)));
        assert_eq!(pace(0.1, Duration::from_millis(200)), Some(Duration::from_millis(500)));
        // Never faster than the agent samples, nor slower than a minute.
        assert_eq!(pace(0.5, Duration::from_secs(2)), Some(Duration::from_secs(2)));
        assert_eq!(pace(1e9, second), Some(Duration::from_secs(60)));
        assert_eq!(pace(f32::NAN, second), None);
        assert_eq!(pace(f32::INFINITY, second), None);
    }

    #[test]
    fn a_viewer_gets_deltas_on_its_pace_and_the_whole_after_a_miss() {
        let (tx, rx) = bounded(1);
        let t0 = Instant::now();
        let every = Duration::from_secs(10);
        let mut v = Viewer { id: 0, tx, want: Want::default(), every, next: t0, base: None };
        let half = Duration::from_millis(500);
        let s = |cpu: f32| Summary { cpu, ..Summary::default() };
        let got = |rx: &Receiver<Frame>| proto::decode::<FromAgent>(&rx.try_recv().unwrap()[4..]).unwrap();
        assert!(v.offer(&s(1.0), t0, half));
        assert_eq!(got(&rx), FromAgent::Summary(s(1.0)));
        // Not due for ten seconds; half a round early is on time.
        assert!(v.offer(&s(2.0), t0 + Duration::from_secs(5), half));
        assert!(rx.is_empty());
        assert!(v.offer(&s(3.0), t0 + Duration::from_millis(9600), half));
        assert!(matches!(got(&rx), FromAgent::Delta(d) if d.cpu == Some(3.0)));
        // One that does not fit is missed, and the next is whole.
        let t = t0 + Duration::from_secs(20);
        v.tx.try_send(Arc::new(Vec::new())).unwrap();
        assert!(v.offer(&s(4.0), t, half));
        assert!(v.base.is_none());
        rx.try_recv().unwrap();
        assert!(v.offer(&s(5.0), t, half));
        assert_eq!(got(&rx), FromAgent::Summary(s(5.0)));
        drop(rx);
        assert!(!v.offer(&s(6.0), t + every, half), "gone");
    }

    #[test]
    fn only_what_its_own_allow_lists_is_done() {
        let m = model();
        let kill = Act::Kill { pid: 42, start: 7 };
        assert_eq!(check(&[], &kill, &m, 1), Err(Refusal::NotAllowed));
        assert_eq!(check(&allow(&["service"]), &kill, &m, 1), Err(Refusal::NotAllowed));
        assert_eq!(check(&allow(&["kill"]), &service(Action::Stop, "cron", false), &m, 1), Err(Refusal::NotAllowed));
        assert_eq!(check(&allow(&["kill"]), &kill, &m, 1), Ok(Job::Kill(kill::Target { pid: 42, name: "sleep".into(), start: 7 })));
    }

    #[test]
    fn the_agent_does_not_end_itself() {
        let m = model();
        assert_eq!(check(&allow(&["kill"]), &Act::Kill { pid: 1, start: 0 }, &m, 1), Err(Refusal::Agent));
        // One it does not list is still tried: the list may be a moment old.
        assert_eq!(
            check(&allow(&["kill"]), &Act::Kill { pid: 9, start: 0 }, &m, 1),
            Ok(Job::Kill(kill::Target { pid: 9, name: String::new(), start: 0 }))
        );
    }

    #[test]
    fn a_service_is_looked_up_in_its_own_list() {
        let (m, a) = (model(), allow(&["service"]));
        assert_eq!(check(&a, &service(Action::Stop, "nope", false), &m, 1), Err(Refusal::NoService));
        // A driver and a service of the same name are not the same thing.
        assert_eq!(check(&a, &service(Action::Stop, "disk", false), &m, 1), Err(Refusal::NoService));
        assert_eq!(check(&a, &service(Action::Stop, "cron", true), &m, 1), Err(Refusal::NoService));
        assert_eq!(
            check(&a, &service(Action::Restart, "disk", true), &m, 1),
            Ok(Job::Svc(svcctl::Target { action: Action::Restart, name: "disk".into(), plist: None, driver: true }))
        );
    }

    #[test]
    fn an_unloaded_job_starts_from_the_plist_this_machine_read() {
        let (m, a) = (model(), allow(&["service"]));
        let plist = Some("/Library/LaunchDaemons/job.plist".to_string());
        assert_eq!(
            check(&a, &service(Action::Start, "job", false), &m, 1),
            Ok(Job::Svc(svcctl::Target { action: Action::Start, name: "job".into(), plist, driver: false }))
        );
        assert_eq!(
            check(&a, &service(Action::Stop, "job", false), &m, 1),
            Ok(Job::Svc(svcctl::Target { action: Action::Stop, name: "job".into(), plist: None, driver: false }))
        );
        assert_eq!(check(&a, &service(Action::Start, "off", false), &m, 1), Err(Refusal::Disabled(svcctl::enable_line("off"))));
    }
}
