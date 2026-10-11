//! The viewer's side: one thread per `[[host]]`, connecting, reading the
//! agent's frames and pinging it, and connecting again after a wait that
//! grows while it stays down. What the viewer wants of a machine (its
//! readings while its screens are open) goes through a channel per host and
//! is said again after each reconnect. What it asks the agent to do goes
//! the same way, once: a request queued while the link was down is dropped,
//! not sent to the next connection.
//!
//! A host with a `key` is reached directly, over Noise ([`super::noise`]),
//! on [`proto::DIRECT_PORT`]; the rest through a tunnel on [`proto::PORT`].
//! The agent sends a whole summary first and then only what changed; the
//! thread keeps the whole and gives the viewer that.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, bounded, never, select, unbounded};

use super::noise::{self, Key, Reader, Refused, Writer};
use super::proto::{self, Act, Done, FromAgent, Hello, PROTO, Summary, ToAgent, Want};
use crate::collect::Msg;
use crate::config::settings::Host;

const CONNECT: Duration = Duration::from_secs(5);
/// The agent sends a summary every interval (10 s at most); nothing for this
/// long is a dead link.
const QUIET: Duration = Duration::from_secs(15);
/// [`probe`]'s wait for the hello: it is this machine.
const PROBE: Duration = Duration::from_secs(2);
const PING_EVERY: Duration = Duration::from_secs(5);
const RETRY_MIN: Duration = Duration::from_secs(2);
const RETRY_MAX: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Up(Hello),
    Summary(Summary),
    /// A ping's round trip.
    Latency(Duration),
    Down(Why),
    /// One of the agent's readings, while the viewer watches it.
    Msg(Box<Msg>),
    /// How the request with this id went.
    Done(u64, Done),
}

/// What the viewer sends a host's thread.
#[derive(Clone, Debug, PartialEq)]
pub enum ToHost {
    /// What it shows of the machine now.
    Want(Want),
    /// Something to do there, answered with [`Event::Done`] and the same id.
    Act(u64, Act),
}

/// Why a host is down, for its card.
#[derive(Clone, Debug, PartialEq)]
pub enum Why {
    /// Nothing listens there: the tunnel is not up.
    Refused,
    /// The name does not resolve.
    Unknown,
    Io(String),
    /// Nothing came for [`QUIET`].
    Quiet,
    /// The other end closed: through a tunnel, the agent is not running.
    Closed,
    /// An agent of another protocol (another yagura version).
    Proto(u16),
    /// Something that is not a yagura agent answered.
    NotAgent,
    /// The agent does not list this machine's key in its `[[agent.viewer]]`.
    Denied,
    /// The host's `key` is not that agent's.
    BadKey,
}

/// How to reach a host.
#[derive(Clone, Debug, Default)]
struct Target {
    addr: String,
    /// The agent's public key: a direct connection.
    key: Option<Key>,
    /// How often it should send a summary, when not every interval.
    every: Option<f32>,
    /// yagura's config folder, where this machine's own key is.
    dir: Option<PathBuf>,
}

impl Target {
    fn of(h: &Host, dir: Option<PathBuf>) -> Self {
        let port = if h.key.is_some() { proto::DIRECT_PORT } else { proto::PORT };
        Self { addr: proto::with_port(&h.addr, port), key: h.key, every: h.every, dir }
    }
}

/// What a host's thread keeps of the viewer's wants.
#[derive(Clone)]
struct Wants {
    rx: Receiver<ToHost>,
    /// The latest, said again after each reconnect.
    now: Arc<Mutex<Want>>,
}

impl Wants {
    fn new(rx: Receiver<ToHost>) -> Self {
        Self { rx, now: Arc::default() }
    }

    fn now(&self) -> Want {
        *self.now.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn set(&self, w: Want) {
        *self.now.lock().unwrap_or_else(|e| e.into_inner()) = w;
    }

    /// Take what was sent while no connection was up: the latest want is
    /// kept, requests are dropped (the viewer has given up on them).
    fn drain(&self) {
        for m in self.rx.try_iter() {
            if let ToHost::Want(w) = m {
                self.set(w);
            }
        }
    }
}

/// Watch every host; events come tagged with the host's index, and each
/// host's sender takes what the viewer wants of it and asks of it. `dir` is
/// yagura's config folder, for this machine's key when a host has one.
pub fn spawn(hosts: &[Host], dir: Option<PathBuf>, wake: Option<egui::Context>) -> (Receiver<(usize, Event)>, Vec<Sender<ToHost>>) {
    let (tx, rx) = unbounded();
    let mut senders = Vec::new();
    for (i, h) in hosts.iter().enumerate() {
        let (want_tx, want_rx) = unbounded();
        senders.push(want_tx);
        let (tx, wake, to) = (tx.clone(), wake.clone(), Target::of(h, dir.clone()));
        let spawned = std::thread::Builder::new().name(format!("yagura-host-{i}")).spawn(move || {
            watch(&to, &Wants::new(want_rx), &|e| {
                let ok = tx.send((i, e)).is_ok();
                if let Some(w) = &wake {
                    w.request_repaint();
                }
                ok
            })
        });
        if let Err(e) = spawned {
            eprintln!("yagura: {}: {e}", h.name);
        }
    }
    (rx, senders)
}

/// Until `emit` says nobody listens.
fn watch(to: &Target, wants: &Wants, emit: &dyn Fn(Event) -> bool) {
    let mut wait = RETRY_MIN;
    loop {
        let Some((was_up, why)) = session(to, wants, emit) else { return };
        if !emit(Event::Down(why)) {
            return;
        }
        if was_up {
            wait = RETRY_MIN;
        }
        std::thread::sleep(wait);
        wait = (wait * 2).min(RETRY_MAX);
    }
}

/// One connection: whether it came up and why it ended; `None` when `emit`
/// failed.
fn session(to: &Target, wants: &Wants, emit: &dyn Fn(Event) -> bool) -> Option<(bool, Why)> {
    let opened = connect(&to.addr).and_then(|s| open(&s, to));
    let (mut r, mut w) = match opened {
        Ok(rw) => rw,
        Err(why) => return Some((false, why)),
    };
    let hello = match greet(&mut r, &mut w) {
        Ok(h) => h,
        Err(why) => return Some((false, why)),
    };
    wants.drain();
    if !emit(Event::Up(hello)) {
        return None;
    }
    let t0 = Instant::now();
    // Dropped when this returns, which stops the writer at once.
    let (_stop, stop_rx) = bounded::<()>(0);
    let (wants2, every) = (wants.clone(), to.every);
    let _ = std::thread::Builder::new().name("yagura-host-out".into()).spawn(move || write(w, t0, every, &wants2, &stop_rx));
    // The whole summary, which each delta changes.
    let mut last: Option<Summary> = None;
    loop {
        let e = match proto::read_frame::<FromAgent>(&mut r) {
            Ok(FromAgent::Summary(sum)) => {
                last = Some(sum.clone());
                Event::Summary(sum)
            }
            Ok(FromAgent::Delta(d)) => match &mut last {
                Some(sum) => {
                    sum.apply(d);
                    Event::Summary(sum.clone())
                }
                // The agent starts each viewer with a whole one.
                None => continue,
            },
            Ok(FromAgent::Pong(n)) => Event::Latency(t0.elapsed().saturating_sub(Duration::from_micros(n))),
            Ok(FromAgent::Hello(_)) => continue,
            Ok(FromAgent::Msg(m)) => Event::Msg(m),
            Ok(FromAgent::Done { id, outcome }) => Event::Done(id, outcome),
            Err(e) => return Some((true, why(&e))),
        };
        if !emit(e) {
            return None;
        }
    }
}

/// The viewer's side of a connection: its pace and what it wants (again,
/// after a reconnect), then each change, and a ping every [`PING_EVERY`].
fn write(mut w: Writer, t0: Instant, every: Option<f32>, wants: &Wants, stop: &Receiver<()>) {
    if let Some(secs) = every
        && proto::write_frame(&mut w, &ToAgent::Pace(secs)).is_err()
    {
        return;
    }
    let now = wants.now();
    if now != Want::default() && proto::write_frame(&mut w, &ToAgent::Watch(now)).is_err() {
        return;
    }
    let mut rx = wants.rx.clone();
    let mut ping = Instant::now();
    loop {
        if Instant::now() >= ping {
            let n = t0.elapsed().as_micros() as u64;
            if proto::write_frame(&mut w, &ToAgent::Ping(n)).is_err() {
                return;
            }
            ping += PING_EVERY;
        }
        select! {
            recv(stop) -> _ => return,
            recv(rx) -> m => match m {
                Ok(ToHost::Want(want)) => {
                    wants.set(want);
                    if proto::write_frame(&mut w, &ToAgent::Watch(want)).is_err() {
                        return;
                    }
                }
                Ok(ToHost::Act(id, act)) => {
                    if proto::write_frame(&mut w, &ToAgent::Act { id, act }).is_err() {
                        return;
                    }
                }
                // The window is gone; the reader stops at its next frame.
                Err(_) => rx = never(),
            },
            default(ping.saturating_duration_since(Instant::now())) => {}
        }
    }
}

/// Whether an agent listens on this machine's `port`: its hello and the
/// summary it sends a new viewer (the latest, made before this joined, so
/// its viewers are the others). For the sys screen; it blocks, so it runs on
/// a thread.
pub fn probe(port: u16) -> Result<(Hello, Summary), Why> {
    // Windows retries a refused connection for about a second before it says so.
    let s = connect(&format!("127.0.0.1:{port}"))?;
    s.set_read_timeout(Some(PROBE)).map_err(|e| why(&e))?;
    let (mut r, mut w) = noise::plain(&s).map_err(|e| why(&e))?;
    let hello = greet(&mut r, &mut w)?;
    // An agent that has not sampled yet sends its first summary within its
    // interval (10 s at most).
    s.set_read_timeout(Some(QUIET)).map_err(|e| why(&e))?;
    loop {
        match proto::read_frame::<FromAgent>(&mut r) {
            Ok(FromAgent::Summary(sum)) => return Ok((hello, sum)),
            Ok(_) => {}
            Err(e) => return Err(why(&e)),
        }
    }
}

/// The addresses a name resolved to, in the order to try them: IPv4
/// first, since an agent's `listen` is usually `0.0.0.0` and a machine name
/// often resolves to an IPv6 link-local address before its IPv4 one. Each
/// once.
fn order(addrs: impl IntoIterator<Item = SocketAddr>) -> Vec<SocketAddr> {
    let mut out: Vec<SocketAddr> = Vec::new();
    for a in addrs {
        if !out.contains(&a) {
            out.push(a);
        }
    }
    out.sort_by_key(|a| a.is_ipv6());
    out
}

fn connect(addr: &str) -> Result<TcpStream, Why> {
    let all = order(addr.to_socket_addrs().map_err(|_| Why::Unknown)?);
    let mut last = Why::Unknown;
    for to in all {
        match TcpStream::connect_timeout(&to, CONNECT) {
            Ok(s) => {
                let _ = s.set_nodelay(true);
                s.set_read_timeout(Some(QUIET)).map_err(|e| why(&e))?;
                return Ok(s);
            }
            Err(e) => last = why(&e),
        }
    }
    Err(last)
}

/// The two halves of a connection: through Noise when the host has a key,
/// plain otherwise.
fn open(s: &TcpStream, to: &Target) -> Result<(Reader, Writer), Why> {
    let Some(agent) = &to.key else { return noise::plain(s).map_err(|e| why(&e)) };
    let dir = to.dir.as_deref().ok_or_else(|| Why::Io("no config folder for this machine's key".into()))?;
    let keys = noise::load_or_make(dir).map_err(|e| Why::Io(e.to_string()))?;
    let rw = noise::initiate(s, &keys, agent).map_err(|f| match f {
        noise::Fail::Refused(Refused::UnknownViewer) => Why::Denied,
        noise::Fail::Refused(Refused::WrongAgentKey) => Why::BadKey,
        noise::Fail::Io(e) => why(&e),
    })?;
    s.set_read_timeout(Some(QUIET)).map_err(|e| why(&e))?;
    Ok(rw)
}

fn greet(r: &mut impl Read, w: &mut impl Write) -> Result<Hello, Why> {
    proto::write_frame(w, &ToAgent::Hello { proto: PROTO }).map_err(|e| why(&e))?;
    let body = proto::read_body(r).map_err(|e| why(&e))?;
    match proto::head(&body) {
        Some((0, p)) if p == PROTO => {}
        Some((0, p)) => return Err(Why::Proto(p)),
        _ => return Err(Why::NotAgent),
    }
    match proto::decode(&body) {
        Ok(FromAgent::Hello(h)) => Ok(h),
        _ => Err(Why::NotAgent),
    }
}

fn why(e: &io::Error) -> Why {
    use io::ErrorKind::*;
    match e.kind() {
        ConnectionRefused => Why::Refused,
        WouldBlock | TimedOut => Why::Quiet,
        UnexpectedEof | ConnectionReset | ConnectionAborted | BrokenPipe => Why::Closed,
        InvalidData => Why::NotAgent,
        _ => Why::Io(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::ViewerKey;
    use crate::remote::agent::{self, Config};
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn a_name_tries_ipv4_first_then_the_rest_once_each() {
        let a = |s: &str| s.parse::<SocketAddr>().unwrap();
        let got = order([a("[fe80::1%3]:7314"), a("192.168.1.5:7314"), a("[::1]:7314"), a("10.0.0.2:7314"), a("192.168.1.5:7314")]);
        assert_eq!(got, [a("192.168.1.5:7314"), a("10.0.0.2:7314"), a("[fe80::1%3]:7314"), a("[::1]:7314")]);
        assert!(order([]).is_empty());
    }

    #[test]
    fn a_dead_address_does_not_stop_the_next_one() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        // "localhost" resolves to ::1 and 127.0.0.1 on most machines; the
        // listener is on IPv4 only, as an agent's `0.0.0.0` is.
        assert!(connect(&format!("localhost:{port}")).is_ok());
    }

    /// One host on `addr`, its events on a channel.
    fn watch_one(addr: String) -> Receiver<Event> {
        watch_with(addr).0
    }

    /// The same, and the sender of what the viewer wants.
    fn watch_with(addr: String) -> (Receiver<Event>, Sender<ToHost>) {
        let (tx, rx) = unbounded();
        let (want_tx, want_rx) = unbounded();
        let to = Target { addr, ..Target::default() };
        std::thread::spawn(move || watch(&to, &Wants::new(want_rx), &|e| tx.send(e).is_ok()));
        (rx, want_tx)
    }

    fn agent(interval: f32) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        let cfg = Config { allow: Vec::new(), interval, direct: None, metrics: None };
        std::thread::spawn(move || agent::serve(l, cfg));
        addr
    }

    /// The readings that come within `d`, ignoring everything else.
    fn readings(rx: &Receiver<Event>, d: Duration) -> Vec<Msg> {
        let end = Instant::now() + d;
        let mut out = Vec::new();
        while let Ok(e) = rx.recv_deadline(end) {
            if let Event::Msg(m) = e {
                out.push(*m);
            }
        }
        out
    }

    fn next(rx: &Receiver<Event>) -> Event {
        rx.recv_timeout(Duration::from_secs(20)).expect("an event")
    }

    #[test]
    fn a_viewer_gets_the_hello_summaries_and_round_trips() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let (addr, port) = (l.local_addr().unwrap().to_string(), l.local_addr().unwrap().port());
        let cfg = Config { allow: vec!["kill".into()], interval: 0.5, direct: None, metrics: None };
        std::thread::spawn(move || agent::serve(l, cfg));
        let rx = watch_one(addr);
        let Event::Up(h) = next(&rx) else { panic!("not up first") };
        assert_eq!((h.proto, h.allow.as_slice(), h.version.as_str()), (PROTO, ["kill".to_string()].as_slice(), env!("CARGO_PKG_VERSION")));
        assert!(!h.host.is_empty() || !h.os.is_empty(), "{h:?}");
        let (mut sum, mut lat) = (None, None);
        while sum.is_none() || lat.is_none() {
            match next(&rx) {
                Event::Summary(s) => sum = Some(s),
                Event::Latency(d) => lat = Some(d),
                e => panic!("{e:?}"),
            }
        }
        let sum = sum.unwrap();
        assert!(sum.mem_total > 0 && sum.mem_used <= sum.mem_total, "{sum:?}");
        assert!(lat.unwrap() < Duration::from_secs(5));
        assert_eq!(h.port, port);
        // The first summary may be from before this viewer joined.
        let counted = std::iter::from_fn(|| Some(next(&rx))).any(|e| matches!(e, Event::Summary(s) if s.viewers == 1));
        assert!(counted, "a later summary counts this viewer");
    }

    #[test]
    fn a_probe_reads_the_agent_on_this_machine() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let cfg = Config { allow: vec!["service".into()], interval: 0.25, direct: None, metrics: None };
        std::thread::spawn(move || agent::serve(l, cfg));
        let (h, sum) = probe(port).expect("an agent");
        assert_eq!((h.port, h.allow.as_slice(), h.pid), (port, ["service".to_string()].as_slice(), std::process::id()));
        assert!(sum.mem_total > 0, "{sum:?}");
    }

    #[test]
    fn a_probe_of_nobody_is_refused() {
        // A port just freed; another test's agent may take it meanwhile.
        let mut got = Vec::new();
        for _ in 0..5 {
            let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
            match probe(port) {
                Err(Why::Refused) => return,
                r => got.push(r.map(|(h, _)| h.port)),
            }
        }
        panic!("never refused: {got:?}");
    }

    #[test]
    fn readings_come_only_while_watched() {
        let (rx, want) = watch_with(agent(0.25));
        assert!(matches!(next(&rx), Event::Up(_)));
        // Not watching: summaries only.
        assert!(readings(&rx, Duration::from_millis(800)).is_empty());
        want.send(ToHost::Want(Want { detail: true, ..Want::default() })).unwrap();
        let got = readings(&rx, Duration::from_secs(3));
        let has = |f: fn(&Msg) -> bool| got.iter().any(f);
        assert!(has(|m| matches!(m, Msg::Static(_))), "the cached static first");
        assert!(has(|m| matches!(m, Msg::Cpu { .. })) && has(|m| matches!(m, Msg::Mem(_))));
        assert!(has(|m| matches!(m, Msg::Procs { .. })), "processes while watched");
        assert!(!has(|m| matches!(m, Msg::Svc(_) | Msg::Conns(_))), "services and sockets only when asked");
        want.send(ToHost::Want(Want { detail: true, conns: true, ..Want::default() })).unwrap();
        assert!(readings(&rx, Duration::from_secs(3)).iter().any(|m| matches!(m, Msg::Conns(_))));
        want.send(ToHost::Want(Want::default())).unwrap();
        // What was on its way when it stopped may still come.
        let _ = readings(&rx, Duration::from_millis(500));
        assert!(readings(&rx, Duration::from_secs(1)).is_empty());
    }

    #[test]
    fn the_want_is_said_again_after_a_reconnect() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        let (said_tx, said_rx) = unbounded();
        std::thread::spawn(move || {
            for s in l.incoming().take(2) {
                let mut s = s.unwrap();
                let _ = proto::read_body(&mut s);
                let _ = proto::write_frame(&mut s, &FromAgent::Hello(Hello { proto: PROTO, ..Hello::default() }));
                // The first watch on each connection; the first connection
                // then closes.
                while let Ok(m) = proto::read_frame::<ToAgent>(&mut s) {
                    if let ToAgent::Watch(w) = m {
                        let _ = said_tx.send(w);
                        break;
                    }
                }
            }
        });
        let (rx, want) = watch_with(addr);
        assert!(matches!(next(&rx), Event::Up(_)));
        let w = Want { detail: true, services: true, conns: false, devices: false, containers: false };
        want.send(ToHost::Want(w)).unwrap();
        assert_eq!(said_rx.recv_timeout(Duration::from_secs(5)), Ok(w));
        assert_eq!(next(&rx), Event::Down(Why::Closed));
        assert!(matches!(next(&rx), Event::Up(_)));
        assert_eq!(said_rx.recv_timeout(Duration::from_secs(5)), Ok(w), "said again unasked");
    }

    /// The answer to request `id`, skipping the summaries and pings.
    fn done(rx: &Receiver<Event>, id: u64) -> Done {
        loop {
            match next(rx) {
                Event::Done(i, d) if i == id => return d,
                Event::Down(why) => panic!("down: {why:?}"),
                _ => {}
            }
        }
    }

    fn agent_allowing(allow: &[&str]) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        let cfg = Config { allow: allow.iter().map(|a| a.to_string()).collect(), interval: 0.5, direct: None, metrics: None };
        std::thread::spawn(move || agent::serve(l, cfg));
        addr
    }

    #[test]
    fn an_agent_refuses_what_its_allow_does_not_list() {
        let mut c = crate::kill::tests::sleeper();
        let (rx, tx) = watch_with(agent_allowing(&[]));
        let Event::Up(h) = next(&rx) else { panic!("not up first") };
        assert_eq!(h.pid, std::process::id(), "the agent runs in this test's process");
        tx.send(ToHost::Act(7, Act::Kill { pid: c.id(), start: 0 })).unwrap();
        assert_eq!(done(&rx, 7), Done::Refused(proto::Refusal::NotAllowed));
        assert!(c.try_wait().unwrap().is_none(), "the child must still be running");
        let _ = c.kill();
        let _ = c.wait();
    }

    #[test]
    fn an_agent_that_allows_kill_ends_the_process() {
        let mut c = crate::kill::tests::sleeper();
        let (rx, tx) = watch_with(agent_allowing(&["kill"]));
        assert!(matches!(next(&rx), Event::Up(_)));
        tx.send(ToHost::Act(1, Act::Kill { pid: c.id(), start: 0 })).unwrap();
        assert_eq!(done(&rx, 1), Done::Kill(crate::kill::Outcome::Ended));
        assert!(!c.wait().unwrap().success());
        // Nor does it end itself, whatever the viewer says.
        tx.send(ToHost::Act(2, Act::Kill { pid: std::process::id(), start: 0 })).unwrap();
        assert_eq!(done(&rx, 2), Done::Refused(proto::Refusal::Agent));
    }

    #[test]
    fn a_request_queued_while_down_is_not_sent_later() {
        let mut c = crate::kill::tests::sleeper();
        // Nothing listens yet: the request waits in the queue.
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        drop(l);
        let (rx, tx) = watch_with(addr.clone());
        assert_eq!(next(&rx), Event::Down(Why::Refused));
        tx.send(ToHost::Act(1, Act::Kill { pid: c.id(), start: 0 })).unwrap();
        let w = Want { detail: true, ..Want::default() };
        tx.send(ToHost::Want(w)).unwrap();
        let l = TcpListener::bind(&addr).unwrap();
        std::thread::spawn(move || agent::serve(l, Config { allow: vec!["kill".into()], interval: 0.25, direct: None, metrics: None }));
        loop {
            match next(&rx) {
                Event::Up(_) => break,
                Event::Down(_) => {}
                e => panic!("{e:?}"),
            }
        }
        // The want went through; the request did not.
        let end = Instant::now() + Duration::from_secs(3);
        let mut readings = false;
        while let Ok(e) = rx.recv_deadline(end) {
            assert!(!matches!(e, Event::Done(..)), "{e:?}");
            readings |= matches!(e, Event::Msg(_));
        }
        assert!(readings, "the want is said once the link is up");
        assert!(c.try_wait().unwrap().is_none(), "the child must still be running");
        let _ = c.kill();
        let _ = c.wait();
    }

    #[test]
    fn something_else_on_the_port_is_not_an_agent() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            for s in l.incoming().take(1) {
                let mut s = s.unwrap();
                let mut buf = [0; 16];
                let _ = s.read(&mut buf);
                let _ = s.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n");
            }
        });
        assert_eq!(next(&watch_one(addr)), Event::Down(Why::NotAgent));
    }

    #[test]
    fn another_protocol_is_said() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            for s in l.incoming().take(1) {
                let mut s = s.unwrap();
                let _ = proto::read_body(&mut s);
                let later = Hello { proto: 99, ..Hello::default() };
                let _ = proto::write_frame(&mut s, &FromAgent::Hello(later));
            }
        });
        assert_eq!(next(&watch_one(addr)), Event::Down(Why::Proto(99)));
    }

    /// An agent that takes viewers with these keys directly: its address
    /// and key.
    fn direct_agent(viewers: Vec<Key>) -> (String, Key) {
        let plain = TcpListener::bind("127.0.0.1:0").unwrap();
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        let keys = noise::Keys::generate().unwrap();
        let public = keys.public;
        let viewers = viewers.into_iter().enumerate().map(|(i, key)| ViewerKey { name: format!("v{i}"), key }).collect();
        let direct = agent::Direct { listen: addr.clone(), keys, viewers };
        let cfg = Config { allow: Vec::new(), interval: 0.25, direct: Some(direct), metrics: None };
        std::thread::spawn(move || agent::serve_all(plain, Some(l), cfg));
        (addr, public)
    }

    /// A viewer with its own key in a new config folder: its public key.
    fn viewer_dir(label: &str) -> (PathBuf, Key) {
        let dir = crate::util::test_dir(label).join("yagura");
        let public = noise::load_or_make(&dir).unwrap().public;
        (dir, public)
    }

    fn watch_target(to: Target) -> Receiver<Event> {
        let (rx, want) = watch_target_with(to);
        std::mem::forget(want);
        rx
    }

    fn watch_target_with(to: Target) -> (Receiver<Event>, Sender<ToHost>) {
        let (tx, rx) = unbounded();
        let (want_tx, want_rx) = unbounded();
        std::thread::spawn(move || watch(&to, &Wants::new(want_rx), &|e| tx.send(e).is_ok()));
        (rx, want_tx)
    }

    #[test]
    fn a_viewer_with_a_key_connects_directly() {
        let (dir, mine) = viewer_dir("direct-ok");
        let (addr, theirs) = direct_agent(vec![mine]);
        let (rx, want) = watch_target_with(Target { addr, key: Some(theirs), every: None, dir: Some(dir) });
        let Event::Up(h) = next(&rx) else { panic!("not up first") };
        assert_eq!((h.key.as_str(), h.proto), (noise::encode(&theirs).as_str(), PROTO));
        assert!(!h.direct.is_empty(), "{h:?}");
        // The summaries come, the whole one first and then the deltas made whole.
        let sums: Vec<Summary> = std::iter::from_fn(|| Some(next(&rx)))
            .filter_map(|e| match e {
                Event::Summary(s) => Some(s),
                Event::Down(why) => panic!("down: {why:?}"),
                _ => None,
            })
            .take(4)
            .collect();
        assert!(sums.iter().all(|s| s.mem_total > 0), "{sums:?}");
        // The process list is larger than one Noise message on most machines.
        want.send(ToHost::Want(Want { detail: true, ..Want::default() })).unwrap();
        assert!(readings(&rx, Duration::from_secs(3)).iter().any(|m| matches!(m, Msg::Procs { .. })), "readings come through Noise");
    }

    #[test]
    fn an_agent_says_it_does_not_know_the_viewer() {
        let (dir, _) = viewer_dir("direct-denied");
        let (addr, theirs) = direct_agent(vec![noise::Keys::generate().unwrap().public]);
        let rx = watch_target(Target { addr, key: Some(theirs), every: None, dir: Some(dir) });
        assert_eq!(next(&rx), Event::Down(Why::Denied));
    }

    #[test]
    fn a_key_of_another_agent_is_said() {
        let (dir, mine) = viewer_dir("direct-bad-key");
        let (addr, _) = direct_agent(vec![mine]);
        let other = noise::Keys::generate().unwrap().public;
        let rx = watch_target(Target { addr, key: Some(other), every: None, dir: Some(dir) });
        assert_eq!(next(&rx), Event::Down(Why::BadKey));
    }

    #[test]
    fn a_plain_viewer_on_the_direct_port_gets_nowhere() {
        let (addr, _) = direct_agent(Vec::new());
        let rx = watch_one(addr);
        assert!(matches!(next(&rx), Event::Down(Why::Closed | Why::NotAgent)));
    }

    /// One request to `addr`, the whole answer.
    fn http(addr: &str, req: &str) -> String {
        use std::io::{Read, Write};
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        s.write_all(req.as_bytes()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    }

    #[test]
    fn the_agent_serves_prometheus_its_page() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let free = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().to_string();
        let cfg = Config { allow: Vec::new(), interval: 0.25, direct: None, metrics: Some(free.clone()) };
        std::thread::spawn(move || agent::serve(l, cfg));
        // The sampler needs a moment before it has a reading to show.
        let mut page = String::new();
        for _ in 0..40 {
            page = http(
                &free,
                "GET /metrics HTTP/1.1
Host: x

",
            );
            if page.contains("yagura_memory_total_bytes") && page.contains("yagura_cpu_usage_percent") {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        assert!(page.starts_with("HTTP/1.1 200 OK"), "{page}");
        assert!(page.contains("text/plain; version=0.0.4"));
        assert!(page.contains("yagura_info{version="), "{page}");
        assert!(
            http(
                &free,
                "GET /other HTTP/1.1

"
            )
            .starts_with("HTTP/1.1 404")
        );
        assert!(
            http(
                &free,
                "POST /metrics HTTP/1.1

"
            )
            .starts_with("HTTP/1.1 404")
        );
    }

    #[test]
    fn a_slow_pace_spaces_the_summaries() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        std::thread::spawn(move || agent::serve(l, Config { allow: Vec::new(), interval: 0.25, direct: None, metrics: None }));
        let rx = watch_target(Target { addr, every: Some(1.0), ..Target::default() });
        assert!(matches!(next(&rx), Event::Up(_)));
        let end = Instant::now() + Duration::from_millis(3300);
        let mut n = 0;
        while let Ok(e) = rx.recv_deadline(end) {
            n += usize::from(matches!(e, Event::Summary(_)));
        }
        // Every 1 s rather than every 0.25 s: the first, then about 3.
        assert!((2..=6).contains(&n), "{n} summaries in 3.3 s");
    }

    #[test]
    fn nobody_listening_is_refused() {
        // A port just freed: nothing listens there.
        let addr = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().to_string();
        assert_eq!(next(&watch_one(addr)), Event::Down(Why::Refused));
    }
}
