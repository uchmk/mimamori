//! The viewer's side: one thread per `[[host]]`, connecting, reading the
//! agent's frames and pinging it, and connecting again after a wait that
//! grows while it stays down.

use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, bounded, unbounded};

use super::proto::{self, FromAgent, Hello, PROTO, Summary, ToAgent};
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

/// Watch every host; events come tagged with the host's index.
pub fn spawn(hosts: &[Host], wake: Option<egui::Context>) -> Receiver<(usize, Event)> {
    let (tx, rx) = unbounded();
    for (i, h) in hosts.iter().enumerate() {
        let (tx, wake, addr) = (tx.clone(), wake.clone(), proto::with_port(&h.addr));
        let spawned = std::thread::Builder::new().name(format!("yagura-host-{i}")).spawn(move || {
            watch(&addr, &|e| {
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
    rx
}

/// Until `emit` says nobody listens.
fn watch(addr: &str, emit: &dyn Fn(Event) -> bool) {
    let mut wait = RETRY_MIN;
    loop {
        let Some((was_up, why)) = session(addr, emit) else { return };
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
fn session(addr: &str, emit: &dyn Fn(Event) -> bool) -> Option<(bool, Why)> {
    let mut s = match connect(addr) {
        Ok(s) => s,
        Err(why) => return Some((false, why)),
    };
    let hello = match greet(&mut s) {
        Ok(h) => h,
        Err(why) => return Some((false, why)),
    };
    if !emit(Event::Up(hello)) {
        return None;
    }
    let t0 = Instant::now();
    // Dropped when this returns, which stops the pinger at once.
    let (_stop, stop_rx) = bounded::<()>(0);
    if let Ok(mut w) = s.try_clone() {
        let _ = std::thread::Builder::new().name("yagura-ping".into()).spawn(move || {
            loop {
                let n = t0.elapsed().as_micros() as u64;
                if proto::write_frame(&mut w, &ToAgent::Ping(n)).is_err() {
                    return;
                }
                if let Err(RecvTimeoutError::Disconnected) = stop_rx.recv_timeout(PING_EVERY) {
                    return;
                }
            }
        });
    }
    loop {
        let e = match proto::read_frame::<FromAgent>(&mut s) {
            Ok(FromAgent::Summary(sum)) => Event::Summary(sum),
            Ok(FromAgent::Pong(n)) => Event::Latency(t0.elapsed().saturating_sub(Duration::from_micros(n))),
            Ok(FromAgent::Hello(_)) => continue,
            Err(e) => return Some((true, why(&e))),
        };
        if !emit(e) {
            return None;
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
        let (tx, rx) = unbounded();
        std::thread::spawn(move || watch(&addr, &|e| tx.send(e).is_ok()));
        rx
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
