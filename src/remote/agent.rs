//! `mimamori --agent`: no window, the same sampler, and a summary for every
//! viewer connected to 127.0.0.1.
//!
//! One hub thread folds the sampler's messages into a [`Model`] and hands
//! each viewer the frames through its own short channel; a viewer that falls
//! behind misses summaries rather than holding the others up.

use std::io;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded, select, unbounded};

use super::proto::{self, FromAgent, Hello, PROTO, Summary, ToAgent};
use crate::collect::{self, Ctl, Msg, gpu};
use crate::model::Model;

/// A frame shared by every viewer.
type Frame = Arc<Vec<u8>>;

/// Frames a viewer may have waiting before it misses some.
const QUEUE: usize = 16;
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
    eprintln!("mimamori {} agent: listening on {} ({what})", env!("CARGO_PKG_VERSION"), listener.local_addr()?);
    eprintln!(
        "mimamori agent: reach it with `ssh -L {port}:localhost:{port} <this host>` or `tailscale serve --bg --tcp {port} tcp://localhost:{port}`"
    );
    serve(listener, cfg)
}

/// Serve viewers on `listener` (the tests pass one on port 0).
pub fn serve(listener: TcpListener, cfg: Config) -> io::Result<()> {
    let (join_tx, join_rx) = unbounded();
    std::thread::Builder::new().name("mimamori-agent".into()).spawn(move || hub(&cfg, &join_rx))?;
    for conn in listener.incoming() {
        match conn {
            Ok(s) => {
                let join = join_tx.clone();
                let spawned = std::thread::Builder::new().name("mimamori-viewer".into()).spawn(move || viewer(s, &join));
                if let Err(e) = spawned {
                    eprintln!("mimamori agent: {e}");
                }
            }
            Err(e) => eprintln!("mimamori agent: {e}"),
        }
    }
    Ok(())
}

fn hub(cfg: &Config, joins: &Receiver<Sender<Frame>>) {
    // `ctl` stays alive here: the sampler stops when it is dropped.
    let (rx, ctl) = collect::spawn(None, Duration::from_secs_f32(cfg.interval));
    let _ = ctl.send(Ctl::Procs(false));
    let mut model = Model::default();
    let mut hello: Option<Frame> = None;
    let mut last: Option<Frame> = None;
    let mut viewers: Vec<Sender<Frame>> = Vec::new();
    loop {
        select! {
            recv(rx) -> m => {
                let Ok(m) = m else { return };
                // The sensors come last in each round: the summary is whole then.
                let (first, round) = (matches!(m, Msg::Static(_)), matches!(m, Msg::Sensors(_)));
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
                viewers.retain(|v| !matches!(v.try_send(f.clone()), Err(TrySendError::Disconnected(_))));
            }
            recv(joins) -> v => {
                let Ok(v) = v else { return };
                // One who comes before the hello gets it with everyone else.
                for f in hello.iter().chain(&last) {
                    let _ = v.try_send(f.clone());
                }
                viewers.push(v);
            }
        }
    }
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
fn viewer(s: TcpStream, join: &Sender<Sender<Frame>>) {
    let peer = s.peer_addr().map(|a| a.to_string()).unwrap_or_default();
    if let Err(e) = talk(&s, join) {
        eprintln!("mimamori agent: {peer}: {e}");
    }
    let _ = s.shutdown(Shutdown::Both);
}

fn talk(s: &TcpStream, join: &Sender<Sender<Frame>>) -> io::Result<()> {
    let _ = s.set_nodelay(true);
    s.set_read_timeout(Some(QUIET))?;
    let mut r = s.try_clone()?;
    let first = proto::read_body(&mut r)?;
    match proto::head(&first) {
        Some((0, p)) if p != PROTO => eprintln!("mimamori agent: a viewer speaks protocol {p}, this agent {PROTO}"),
        Some((0, _)) => {}
        _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "not a mimamori viewer")),
    }
    let (tx, rx) = bounded::<Frame>(QUEUE);
    let mut w = s.try_clone()?;
    std::thread::Builder::new().name("mimamori-viewer-out".into()).spawn(move || {
        for f in rx {
            if io::Write::write_all(&mut w, &f).is_err() {
                break;
            }
        }
    })?;
    if join.send(tx.clone()).is_err() {
        return Ok(());
    }
    loop {
        match proto::read_frame::<ToAgent>(&mut r)? {
            ToAgent::Ping(n) => {
                if let Err(TrySendError::Disconnected(_)) = tx.try_send(Arc::new(proto::frame(&FromAgent::Pong(n)))) {
                    return Ok(());
                }
            }
            ToAgent::Hello { .. } => {}
        }
    }
}
