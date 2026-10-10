//! The fleet: other machines' mimamori, each run as `mimamori --agent`.
//!
//! The agent listens on 127.0.0.1 only and has no keys or passwords of its
//! own: the viewer reaches it through `ssh -L` or `tailscale serve`, which
//! already say who may connect and encrypt the way. [`proto`] is what they
//! say, [`agent`] the listening side, [`client`] one thread per host on the
//! viewer's side, and [`Peer`] what the fleet screen draws from.

pub mod agent;
pub mod client;
pub mod proto;

use std::time::{Duration, Instant, SystemTime};

use crate::config::settings::Host;
use crate::model::Ring;
use client::{Event, Why};
use proto::{Hello, Summary};

/// A round trip slower than this marks the card slow.
pub const SLOW: Duration = Duration::from_millis(200);

#[derive(Clone, Debug, PartialEq)]
pub enum Link {
    Connecting,
    Up,
    Down(Why),
}

/// The dot on a card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    /// Up, but the round trip is slow or the summaries stopped coming.
    Slow,
    Down,
    Connecting,
}

impl Status {
    pub fn key(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Slow => "slow",
            Status::Down => "down",
            Status::Connecting => "connecting",
        }
    }
}

/// One `[[host]]` as the viewer knows it.
#[derive(Debug)]
pub struct Peer {
    pub name: String,
    pub addr: String,
    pub link: Link,
    /// The agent's, from the last connection (kept while it is down).
    pub hello: Option<Hello>,
    pub latency: Option<Duration>,
    pub summary: Option<Summary>,
    /// When the last news came (the hello or a summary).
    pub at: Option<Instant>,
    /// The wall clock of the last summary, for "last seen".
    pub seen: Option<SystemTime>,
    pub cpu: Ring,
    pub mem: Ring,
    pub gpu: Ring,
}

impl Peer {
    pub fn new(h: &Host) -> Self {
        Self {
            name: h.name.clone(),
            addr: h.addr.clone(),
            link: Link::Connecting,
            hello: None,
            latency: None,
            summary: None,
            at: None,
            seen: None,
            cpu: Ring::default(),
            mem: Ring::default(),
            gpu: Ring::default(),
        }
    }

    pub fn apply(&mut self, e: Event) {
        match e {
            Event::Up(h) => {
                self.link = Link::Up;
                self.hello = Some(h);
                self.latency = None;
                self.at = Some(Instant::now());
            }
            Event::Summary(s) => {
                self.cpu.push(s.cpu);
                self.mem.push(s.mem_pct());
                self.gpu.push(s.gpu.as_ref().map_or(0.0, |g| g.1));
                self.summary = Some(s);
                self.at = Some(Instant::now());
                self.seen = Some(SystemTime::now());
            }
            Event::Latency(d) => self.latency = Some(d),
            Event::Down(why) => self.link = Link::Down(why),
        }
    }

    /// Seconds between the agent's summaries (1 until it says).
    pub fn interval(&self) -> Duration {
        Duration::from_secs_f32(self.hello.as_ref().map_or(1.0, |h| h.interval).max(0.1))
    }

    pub fn status(&self, now: Instant) -> Status {
        match self.link {
            Link::Connecting => Status::Connecting,
            Link::Down(_) => Status::Down,
            Link::Up => {
                let late = self.at.is_some_and(|t| now.saturating_duration_since(t) > self.interval() * 3);
                if late || self.latency.is_some_and(|l| l >= SLOW) { Status::Slow } else { Status::Ok }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer() -> Peer {
        Peer::new(&Host { name: "nas".into(), addr: "localhost:7314".into() })
    }

    #[test]
    fn a_peer_follows_its_connection() {
        let mut p = peer();
        let now = Instant::now();
        assert_eq!(p.status(now), Status::Connecting);
        p.apply(Event::Up(Hello { interval: 1.0, ..Hello::default() }));
        assert_eq!(p.status(Instant::now()), Status::Ok);
        p.apply(Event::Summary(Summary { cpu: 30.0, mem_used: 4, mem_total: 16, gpu: Some(("GPU".into(), 50.0)), temp: None }));
        assert_eq!((p.cpu.last(), p.mem.last(), p.gpu.last()), (Some(30.0), Some(25.0), Some(50.0)));
        assert!(p.seen.is_some());
        p.apply(Event::Latency(Duration::from_millis(250)));
        assert_eq!(p.status(Instant::now()), Status::Slow);
        p.apply(Event::Latency(Duration::from_millis(3)));
        assert_eq!(p.status(Instant::now()), Status::Ok);
        // Three intervals without a summary is slow.
        assert_eq!(p.status(Instant::now() + Duration::from_secs(4)), Status::Slow);
        // Down keeps the last numbers for the dimmed card.
        p.apply(Event::Down(Why::Closed));
        assert_eq!(p.status(Instant::now()), Status::Down);
        assert!(p.summary.is_some() && p.hello.is_some());
    }
}
