//! `yagura --agent`: no window, the same sampler, and a summary for every
//! viewer connected to 127.0.0.1.
//!
//! One hub thread folds the sampler's messages into a [`Model`] and hands
//! each viewer the frames through its own short channel; a viewer that falls
//! behind misses summaries rather than holding the others up. A viewer that
//! watches the machine (`ToAgent::Watch`) also gets the sampler's readings,
//! starting with the latest of each; processes, services and sockets are
//! read only while somebody wants them.

use std::io;
use std::mem::Discriminant;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded, select, unbounded};

use super::proto::{self, FromAgent, Hello, PROTO, Summary, ToAgent, Want};
use super::wire::{self, Route};
use crate::collect::{self, Ctl, Msg, gpu};
use crate::model::Model;

/// A frame shared by every viewer.
type Frame = Arc<Vec<u8>>;

/// Frames a viewer may have waiting before it misses some.
const QUEUE: usize = 64;
/// A viewer pings every few seconds; one silent this long is gone.
const QUIET: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
pub struct Config {
    /// `[agent] allow`.
    pub allow: Vec<String>,
    /// Seconds between samples.
    pub interval: f32,
}

/// Listen on `127.0.0.1:port` until killed.
pub fn run(port: u16, cfg: Config) -> io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let what = if cfg.allow.is_empty() { "view only".to_string() } else { format!("allows {}", cfg.allow.join(", ")) };
    eprintln!("yagura {} agent: listening on {} ({what})", env!("CARGO_PKG_VERSION"), listener.local_addr()?);
    eprintln!(
        "yagura agent: reach it with `ssh -L {port}:localhost:{port} <this host>` or `tailscale serve --bg --tcp {port} tcp://localhost:{port}`"
    );
    serve(listener, cfg)
}

/// Serve viewers on `listener` (the tests pass one on port 0).
pub fn serve(listener: TcpListener, cfg: Config) -> io::Result<()> {
    let (hub_tx, hub_rx) = unbounded();
    std::thread::Builder::new().name("yagura-agent".into()).spawn(move || hub(&cfg, &hub_rx))?;
    for conn in listener.incoming() {
        match conn {
            Ok(s) => {
                let hub = hub_tx.clone();
                let spawned = std::thread::Builder::new().name("yagura-viewer".into()).spawn(move || viewer(s, &hub));
                if let Err(e) = spawned {
                    eprintln!("yagura agent: {e}");
                }
            }
            Err(e) => eprintln!("yagura agent: {e}"),
        }
    }
    Ok(())
}

/// What a viewer thread tells the hub.
enum HubIn {
    Join(u64, Sender<Frame>),
    Want(u64, Want),
    Leave(u64),
}

struct Viewer {
    id: u64,
    tx: Sender<Frame>,
    want: Want,
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

fn hub(cfg: &Config, ins: &Receiver<HubIn>) {
    // `ctl` stays alive here: the sampler stops when it is dropped.
    let (rx, ctl) = collect::spawn(None, Duration::from_secs_f32(cfg.interval));
    // Processes, services and sockets only while somebody watches.
    let _ = ctl.send(Ctl::Procs(0));
    let mut reading = Want::default();
    let procs_kind = std::mem::discriminant(&Msg::Procs { procs: Vec::new(), total_threads: None });
    let mut model = Model::default();
    let mut hello: Option<Frame> = None;
    let mut last: Option<Frame> = None;
    let mut cache = Cache::default();
    let mut viewers: Vec<Viewer> = Vec::new();
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
                let f = if first {
                    let f: Frame = Arc::new(proto::frame(&FromAgent::Hello(hello_of(&model, cfg))));
                    hello = Some(f.clone());
                    f
                } else if round {
                    let f: Frame = Arc::new(proto::frame(&FromAgent::Summary(summary(&model))));
                    last = Some(f.clone());
                    f
                } else {
                    continue;
                };
                send(&mut viewers, &f, |_| true);
            }
            recv(ins) -> m => {
                match m {
                    Ok(HubIn::Join(id, tx)) => {
                        // One who comes before the hello gets it with everyone else.
                        for f in hello.iter().chain(&last) {
                            let _ = tx.try_send(f.clone());
                        }
                        viewers.push(Viewer { id, tx, want: Want::default() });
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
                    Err(_) => return,
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

/// `f` to every viewer `to` picks; those gone are dropped.
fn send(viewers: &mut Vec<Viewer>, f: &Frame, to: impl Fn(Want) -> bool) {
    viewers.retain(|v| !to(v.want) || !matches!(v.tx.try_send(f.clone()), Err(TrySendError::Disconnected(_))));
}

fn hello_of(m: &Model, cfg: &Config) -> Hello {
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
    }
}

/// The card's numbers from the latest readings.
pub fn summary(m: &Model) -> Summary {
    let gpu = m.gpus.iter().filter(|d| d.kind == gpu::Kind::Gpu).find_map(|d| {
        let name = if d.name.is_empty() { d.vendor.unwrap_or("GPU").to_string() } else { d.name.clone() };
        d.usage.map(|u| (name, u))
    });
    Summary { cpu: m.total, mem_used: m.mem.used, mem_total: m.mem.total, gpu, temp: m.sensors.hottest().map(|s| s.value) }
}

/// One viewer: its hello, then frames out and pings in until either side
/// stops.
fn viewer(s: TcpStream, hub: &Sender<HubIn>) {
    let peer = s.peer_addr().map(|a| a.to_string()).unwrap_or_default();
    if let Err(e) = talk(&s, hub) {
        eprintln!("yagura agent: {peer}: {e}");
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

fn talk(s: &TcpStream, hub: &Sender<HubIn>) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let _ = s.set_nodelay(true);
    s.set_read_timeout(Some(QUIET))?;
    let mut r = s.try_clone()?;
    let first = proto::read_body(&mut r)?;
    match proto::head(&first) {
        Some((0, p)) if p != PROTO => eprintln!("yagura agent: a viewer speaks protocol {p}, this agent {PROTO}"),
        Some((0, _)) => {}
        _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "not a yagura viewer")),
    }
    let (tx, rx) = bounded::<Frame>(QUEUE);
    let mut w = s.try_clone()?;
    std::thread::Builder::new().name("yagura-viewer-out".into()).spawn(move || {
        for f in rx {
            if io::Write::write_all(&mut w, &f).is_err() {
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
            ToAgent::Hello { .. } => {}
        }
    }
}
