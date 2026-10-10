//! The viewer's side: one thread per `[[host]]`, connecting, reading the
//! agent's frames and pinging it, and connecting again after a wait that
//! grows while it stays down. What the viewer wants of a machine (its
//! readings while its screens are open) goes through a channel per host and
//! is said again after each reconnect. What it asks the agent to do goes
//! the same way, once: a request queued while the link was down is dropped,
//! not sent to the next connection.

use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, bounded, never, select, unbounded};

use super::proto::{self, Act, Done, FromAgent, Hello, PROTO, Summary, ToAgent, Want};
use crate::collect::Msg;
use crate::config::settings::Host;

const CONNECT: Duration = Duration::from_secs(5);
/// The agent sends a summary every interval (10 s at most); nothing for this
/// long is a dead link.
const QUIET: Duration = Duration::from_secs(15);
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
/// host's sender takes what the viewer wants of it and asks of it.
pub fn spawn(hosts: &[Host], wake: Option<egui::Context>) -> (Receiver<(usize, Event)>, Vec<Sender<ToHost>>) {
    let (tx, rx) = unbounded();
    let mut senders = Vec::new();
    for (i, h) in hosts.iter().enumerate() {
        let (want_tx, want_rx) = unbounded();
        senders.push(want_tx);
        let (tx, wake, addr) = (tx.clone(), wake.clone(), proto::with_port(&h.addr));
        let spawned = std::thread::Builder::new().name(format!("yagura-host-{i}")).spawn(move || {
            watch(&addr, &Wants::new(want_rx), &|e| {
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
fn watch(addr: &str, wants: &Wants, emit: &dyn Fn(Event) -> bool) {
    let mut wait = RETRY_MIN;
    loop {
        let Some((was_up, why)) = session(addr, wants, emit) else { return };
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
fn session(addr: &str, wants: &Wants, emit: &dyn Fn(Event) -> bool) -> Option<(bool, Why)> {
    let mut s = match connect(addr) {
        Ok(s) => s,
        Err(why) => return Some((false, why)),
    };
    let hello = match greet(&mut s) {
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
    if let Ok(w) = s.try_clone() {
        let wants = wants.clone();
        let _ = std::thread::Builder::new().name("yagura-host-out".into()).spawn(move || write(w, t0, &wants, &stop_rx));
    }
    loop {
        let e = match proto::read_frame::<FromAgent>(&mut s) {
            Ok(FromAgent::Summary(sum)) => Event::Summary(sum),
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

/// The viewer's side of a connection: what it wants (again, after a
/// reconnect), then each change, and a ping every [`PING_EVERY`].
fn write(mut w: TcpStream, t0: Instant, wants: &Wants, stop: &Receiver<()>) {
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

fn connect(addr: &str) -> Result<TcpStream, Why> {
    let to = addr.to_socket_addrs().map_err(|_| Why::Unknown)?.next().ok_or(Why::Unknown)?;
    let s = TcpStream::connect_timeout(&to, CONNECT).map_err(|e| why(&e))?;
    let _ = s.set_nodelay(true);
    s.set_read_timeout(Some(QUIET)).map_err(|e| why(&e))?;
    Ok(s)
}

fn greet(s: &mut TcpStream) -> Result<Hello, Why> {
    proto::write_frame(s, &ToAgent::Hello { proto: PROTO }).map_err(|e| why(&e))?;
    let body = proto::read_body(s).map_err(|e| why(&e))?;
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
    use crate::remote::agent::{self, Config};
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// One host on `addr`, its events on a channel.
    fn watch_one(addr: String) -> Receiver<Event> {
        watch_with(addr).0
    }

    /// The same, and the sender of what the viewer wants.
    fn watch_with(addr: String) -> (Receiver<Event>, Sender<ToHost>) {
        let (tx, rx) = unbounded();
        let (want_tx, want_rx) = unbounded();
        std::thread::spawn(move || watch(&addr, &Wants::new(want_rx), &|e| tx.send(e).is_ok()));
        (rx, want_tx)
    }

    fn agent(interval: f32) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        let cfg = Config { allow: Vec::new(), interval };
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
        let addr = l.local_addr().unwrap().to_string();
        let cfg = Config { allow: vec!["kill".into()], interval: 0.5 };
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
        let w = Want { detail: true, services: true, conns: false };
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
        let cfg = Config { allow: allow.iter().map(|a| a.to_string()).collect(), interval: 0.5 };
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
        std::thread::spawn(move || agent::serve(l, Config { allow: vec!["kill".into()], interval: 0.25 }));
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

    #[test]
    fn nobody_listening_is_refused() {
        // A port just freed: nothing listens there.
        let addr = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().to_string();
        assert_eq!(next(&watch_one(addr)), Event::Down(Why::Refused));
    }
}
