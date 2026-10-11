//! What the fleet agent and viewer say to each other: postcard messages, each
//! framed as a little-endian `u32` length and the bytes. Pure, so every OS
//! tests it.
//!
//! The first message each way is a `Hello` whose first field is the protocol
//! number. Both stay first (the variant and the field), so [`head`] reads
//! them from a peer of any version and a mismatch is said rather than
//! misread.
//!
//! On a thin line the summaries after the first are [`Delta`]s, and a viewer
//! may ask for them less often ([`ToAgent::Pace`]).

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::collect::Msg;
pub use crate::collect::battery::Battery;
use crate::{kill, svcctl};

/// Where the agent listens on 127.0.0.1 unless `[agent] port` says otherwise.
pub const PORT: u16 = 7313;
/// The port of `[agent] listen`, the direct (Noise) one, when it names none.
pub const DIRECT_PORT: u16 = 7314;
/// Raised whenever a message changes shape.
pub const PROTO: u16 = 7;
/// A frame larger than this is not ours (another service on the port). The
/// process list of a busy machine is the largest that is.
pub const MAX_FRAME: usize = 16 << 20;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ToAgent {
    Hello {
        proto: u16,
    },
    /// Answered with [`FromAgent::Pong`] carrying the same number.
    Ping(u64),
    /// What the viewer shows of this machine now; the default (nothing) is
    /// the fleet card alone.
    Watch(Want),
    /// Do something on the agent's machine, if its `[agent] allow` lets
    /// viewers; answered with [`FromAgent::Done`] carrying the same id.
    Act {
        id: u64,
        act: Act,
    },
    /// Seconds between this viewer's summaries, for a thin line; the agent
    /// keeps it within [`PACE_MIN`] and [`PACE_MAX`] and never goes faster
    /// than its own interval.
    Pace(f32),
}

pub const PACE_MIN: f32 = 0.5;
pub const PACE_MAX: f32 = 60.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FromAgent {
    Hello(Hello),
    /// Every sampling interval.
    Summary(Summary),
    Pong(u64),
    /// A sampler reading, while the viewer [watches](ToAgent::Watch).
    Msg(Box<Msg>),
    /// How an [`ToAgent::Act`] went.
    Done {
        id: u64,
        outcome: Done,
    },
    /// What changed since the summary or delta before it on this connection.
    Delta(Delta),
}

/// What a viewer asks the agent to do. The agent checks it against its own
/// `[agent] allow` and its own readings: a service's plist, for one, is
/// looked up there rather than taken from the viewer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Act {
    /// End a process; `start` as the viewer's list showed it, so a PID taken
    /// over meanwhile is left alone.
    Kill {
        pid: u32,
        start: u64,
    },
    Service {
        action: svcctl::Action,
        name: String,
        driver: bool,
    },
}

impl Act {
    /// The `[agent] allow` word that lets it.
    pub fn word(&self) -> &'static str {
        match self {
            Act::Kill { .. } => "kill",
            Act::Service { .. } => "service",
        }
    }
}

/// How an [`Act`] went.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Done {
    Kill(kill::Outcome),
    Svc(svcctl::Outcome),
    /// The agent would not try.
    Refused(Refusal),
}

/// Why the agent would not try.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Refusal {
    /// Its `[agent] allow` does not list it.
    NotAllowed,
    /// It reads no such service.
    NoService,
    /// The process is the agent itself.
    Agent,
    /// A launchd job that is disabled; the command that enables it.
    Disabled(String),
}

/// What a viewer shows of the agent's machine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Want {
    /// Inside the machine's screens: every reading they draw from.
    pub detail: bool,
    /// On its svc screen.
    pub services: bool,
    /// On its net screen.
    pub conns: bool,
    /// On its devices screen.
    pub devices: bool,
    /// On its containers screen.
    pub containers: bool,
}

/// Who the agent is, sent once when the viewer connects.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub proto: u16,
    /// yagura's version on that machine.
    pub version: String,
    pub host: String,
    pub os: String,
    pub arch: String,
    pub brand: String,
    /// Seconds since the epoch.
    pub boot_time: u64,
    /// The account the agent runs as (what it could kill or restart).
    pub user: String,
    /// `[agent] allow` there; empty is view only.
    pub allow: Vec<String>,
    /// Seconds between summaries.
    pub interval: f32,
    /// The agent's own process, which it does not end.
    pub pid: u32,
    /// The port it listens on, on 127.0.0.1.
    pub port: u16,
    /// `[agent] listen`, where viewers with a key connect directly; empty
    /// when it does not.
    pub direct: String,
    /// The agent's public key (base64) when it listens directly.
    pub key: String,
}

/// The card's numbers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    /// Percent of the whole CPU.
    pub cpu: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    /// The first GPU's name and busy percent.
    pub gpu: Option<(String, f32)>,
    /// The hottest temperature, °C.
    pub temp: Option<f32>,
    /// A laptop's; `None` without a battery.
    pub battery: Option<Battery>,
    pub warn: Warn,
    /// Viewers connected to the agent when the summary was made (0 on the
    /// window's own card). The one a new viewer gets first was made before
    /// it joined.
    pub viewers: u16,
}

/// What on the machine wants a look: the counts its screens draw in yellow
/// or red.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warn {
    /// Temperatures at or past their limit.
    pub hot: u16,
    /// Drives whose health, SMART counts or wear warn.
    pub drives: u16,
    /// Services that failed; `None` until services are read.
    pub failed: Option<u16>,
}

impl Warn {
    pub fn total(&self) -> u32 {
        u32::from(self.hot) + u32::from(self.drives) + u32::from(self.failed.unwrap_or(0))
    }
}

impl Summary {
    pub fn mem_pct(&self) -> f32 {
        if self.mem_total == 0 { 0.0 } else { self.mem_used as f32 / self.mem_total as f32 * 100.0 }
    }

    /// What `new` changes of `self`.
    pub fn diff(&self, new: &Summary) -> Delta {
        fn changed<T: PartialEq + Clone>(old: &T, new: &T) -> Option<T> {
            (old != new).then(|| new.clone())
        }
        let name = |g: &Option<(String, f32)>| g.as_ref().map(|g| g.0.clone());
        let busy = |g: &Option<(String, f32)>| g.as_ref().map(|g| g.1);
        Delta {
            cpu: changed(&self.cpu, &new.cpu),
            mem_used: changed(&self.mem_used, &new.mem_used),
            mem_total: changed(&self.mem_total, &new.mem_total),
            gpu_name: changed(&name(&self.gpu), &name(&new.gpu)),
            gpu_busy: changed(&busy(&self.gpu), &busy(&new.gpu)).flatten(),
            temp: changed(&self.temp, &new.temp),
            battery: changed(&self.battery, &new.battery),
            warn: changed(&self.warn, &new.warn),
            viewers: changed(&self.viewers, &new.viewers),
        }
    }

    /// `self` with `d` applied: the summary [`diff`](Self::diff) was made
    /// against becomes the one it was made from.
    pub fn apply(&mut self, d: Delta) {
        if let Some(v) = d.cpu {
            self.cpu = v;
        }
        if let Some(v) = d.mem_used {
            self.mem_used = v;
        }
        if let Some(v) = d.mem_total {
            self.mem_total = v;
        }
        if let Some(name) = d.gpu_name {
            let busy = self.gpu.as_ref().map_or(0.0, |g| g.1);
            self.gpu = name.map(|n| (n, busy));
        }
        if let (Some(v), Some(g)) = (d.gpu_busy, self.gpu.as_mut()) {
            g.1 = v;
        }
        if let Some(v) = d.temp {
            self.temp = v;
        }
        if let Some(v) = d.battery {
            self.battery = v;
        }
        if let Some(v) = d.warn {
            self.warn = v;
        }
        if let Some(v) = d.viewers {
            self.viewers = v;
        }
    }
}

/// A [`Summary`]'s changes: `None` is unchanged, so a quiet machine's delta
/// is a few bytes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Delta {
    pub cpu: Option<f32>,
    pub mem_used: Option<u64>,
    pub mem_total: Option<u64>,
    /// The first GPU's name, or that there is none now.
    pub gpu_name: Option<Option<String>>,
    pub gpu_busy: Option<f32>,
    pub temp: Option<Option<f32>>,
    pub battery: Option<Option<Battery>>,
    pub warn: Option<Warn>,
    pub viewers: Option<u16>,
}

/// One message as a frame.
pub fn frame<T: Serialize>(msg: &T) -> Vec<u8> {
    let body = postcard::to_stdvec(msg).expect("postcard writes to a Vec");
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

pub fn write_frame<T: Serialize>(w: &mut impl Write, msg: &T) -> io::Result<()> {
    w.write_all(&frame(msg))?;
    w.flush()
}

/// The next frame's bytes (without the length).
pub fn read_body(r: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut len = [0; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("a {len} byte frame: not a yagura agent")));
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    Ok(body)
}

pub fn decode<T: DeserializeOwned>(body: &[u8]) -> io::Result<T> {
    postcard::from_bytes(body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn read_frame<T: DeserializeOwned>(r: &mut impl Read) -> io::Result<T> {
    decode(&read_body(r)?)
}

/// The variant and the protocol number of a first message, whatever version
/// sent it: `Some((0, proto))` for a `Hello`.
pub fn head(body: &[u8]) -> Option<(u32, u16)> {
    postcard::take_from_bytes::<(u32, u16)>(body).ok().map(|(h, _)| h)
}

/// `addr` with `port` when it names none: `nas` → `nas:7313`, `::1` and
/// `[::1]` → `[::1]:7313`.
pub fn with_port(addr: &str, port: u16) -> String {
    let addr = addr.trim();
    if let Some(rest) = addr.strip_prefix('[') {
        // `[v6]` or `[v6]:port`.
        return if rest.contains("]:") { addr.to_string() } else { format!("{addr}:{port}") };
    }
    match addr.matches(':').count() {
        0 => format!("{addr}:{port}"),
        1 => addr.to_string(),
        // A bare IPv6 address.
        _ => format!("[{addr}]:{port}"),
    }
}

/// Whether `allow` (an agent's `[agent] allow`) lets a viewer do `action`.
pub fn allowed(allow: &[String], action: &str) -> bool {
    allow.iter().any(|a| a == action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_go_round() {
        let hello = Hello { proto: PROTO, version: "0.31.0".into(), host: "nas".into(), allow: vec!["kill".into()], ..Hello::default() };
        let hello = Hello { port: 7314, direct: "0.0.0.0:7314".into(), key: "abc=".into(), ..hello };
        let sum = Summary {
            cpu: 12.5,
            mem_used: 3 << 30,
            mem_total: 16 << 30,
            gpu: Some(("RTX 4070".into(), 40.0)),
            temp: Some(61.0),
            battery: Some(Battery { pct: 87.0, charging: true, plugged: true }),
            warn: Warn { hot: 1, drives: 0, failed: Some(2) },
            viewers: 3,
        };
        assert_eq!(sum.warn.total(), 3);
        let mut wire = Vec::new();
        write_frame(&mut wire, &FromAgent::Hello(hello.clone())).unwrap();
        write_frame(&mut wire, &FromAgent::Summary(sum.clone())).unwrap();
        write_frame(&mut wire, &FromAgent::Pong(7)).unwrap();
        write_frame(&mut wire, &FromAgent::Delta(Delta { cpu: Some(3.0), ..Delta::default() })).unwrap();
        let mut r = wire.as_slice();
        let first = read_body(&mut r).unwrap();
        assert_eq!(head(&first), Some((0, PROTO)));
        assert_eq!(decode::<FromAgent>(&first).unwrap(), FromAgent::Hello(hello));
        assert_eq!(read_frame::<FromAgent>(&mut r).unwrap(), FromAgent::Summary(sum));
        assert_eq!(read_frame::<FromAgent>(&mut r).unwrap(), FromAgent::Pong(7));
        assert!(matches!(read_frame::<FromAgent>(&mut r).unwrap(), FromAgent::Delta(d) if d.cpu == Some(3.0)));
        assert!(read_frame::<FromAgent>(&mut r).is_err(), "nothing left");
        assert_eq!(Warn::default().total(), 0);
    }

    #[test]
    fn acts_and_their_answers_go_round() {
        let acts = [
            ToAgent::Act { id: 1, act: Act::Kill { pid: 42, start: 1_700_000_000 } },
            ToAgent::Act { id: 2, act: Act::Service { action: svcctl::Action::Restart, name: "cron".into(), driver: false } },
            ToAgent::Pace(10.0),
        ];
        let dones = [
            FromAgent::Done { id: 1, outcome: Done::Kill(kill::Outcome::Failed("Operation not permitted".into())) },
            FromAgent::Done { id: 2, outcome: Done::Svc(svcctl::Outcome::Denied) },
            FromAgent::Done { id: 3, outcome: Done::Refused(Refusal::Disabled("launchctl enable gui/501/x".into())) },
        ];
        for a in acts {
            assert_eq!(decode::<ToAgent>(&frame(&a)[4..]).unwrap(), a);
        }
        for d in dones {
            assert_eq!(decode::<FromAgent>(&frame(&d)[4..]).unwrap(), d);
        }
        assert_eq!(Act::Kill { pid: 1, start: 0 }.word(), "kill");
    }

    #[test]
    fn the_head_is_read_from_any_version() {
        let body = &frame(&ToAgent::Hello { proto: PROTO })[4..];
        assert_eq!(head(body), Some((0, PROTO)));
        // A later Hello with more after the number still shows its number.
        #[derive(Serialize)]
        enum Later {
            Hello { proto: u16, extra: String },
        }
        let body = &frame(&Later::Hello { proto: 9, extra: "new".into() })[4..];
        assert_eq!(head(body), Some((0, 9)));
        assert_eq!(head(&frame(&ToAgent::Ping(3))[4..]).map(|h| h.0), Some(1));
        assert_eq!(head(&[]), None);
    }

    #[test]
    fn a_huge_frame_is_not_ours() {
        // An HTTP server on the port answers with text, read as a length.
        let mut r: &[u8] = b"HTTP/1.1 400 Bad Request\r\n";
        let e = read_body(&mut r).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn the_port_is_added_when_missing() {
        assert_eq!(with_port("nas", PORT), "nas:7313");
        assert_eq!(with_port(" localhost:7314 ", PORT), "localhost:7314");
        assert_eq!(with_port("100.64.0.2", PORT), "100.64.0.2:7313");
        assert_eq!(with_port("::1", PORT), "[::1]:7313");
        assert_eq!(with_port("[::1]", PORT), "[::1]:7313");
        assert_eq!(with_port("[fe80::1]:9000", PORT), "[fe80::1]:9000");
        assert_eq!(with_port("nas", DIRECT_PORT), "nas:7314");
    }

    fn busy() -> Summary {
        Summary {
            cpu: 12.5,
            mem_used: 3 << 30,
            mem_total: 16 << 30,
            gpu: Some(("RTX 4070".into(), 40.0)),
            temp: Some(61.0),
            battery: Some(Battery { pct: 87.0, charging: true, plugged: true }),
            warn: Warn { hot: 1, drives: 0, failed: Some(2) },
            viewers: 1,
        }
    }

    #[test]
    fn a_delta_turns_the_old_summary_into_the_new() {
        let old = busy();
        let cases = [
            Summary { cpu: 50.0, mem_used: 4 << 30, ..busy() },
            Summary { gpu: Some(("RTX 4070".into(), 90.0)), temp: None, ..busy() },
            Summary { gpu: None, battery: None, warn: Warn::default(), viewers: 2, ..busy() },
            Summary { gpu: Some(("Arc".into(), 40.0)), mem_total: 32 << 30, ..busy() },
            Summary::default(),
        ];
        for new in cases {
            let mut got = old.clone();
            got.apply(old.diff(&new));
            assert_eq!(got, new);
            // And back, from a machine with no GPU to one with.
            let mut back = new.clone();
            back.apply(new.diff(&old));
            assert_eq!(back, old);
        }
        assert_eq!(old.diff(&old), Delta::default());
    }

    #[test]
    fn a_delta_is_smaller_than_the_summary() {
        let old = busy();
        let quiet = frame(&FromAgent::Delta(old.diff(&old))).len();
        let full = frame(&FromAgent::Summary(old.clone())).len();
        let cpu = frame(&FromAgent::Delta(old.diff(&Summary { cpu: 13.0, ..busy() }))).len();
        assert!(quiet <= 4 + 1 + 9, "{quiet}");
        assert!(cpu < full / 2, "{cpu} vs {full}");
    }

    #[test]
    fn only_what_is_allowed() {
        let allow = vec!["kill".to_string()];
        assert!(allowed(&allow, "kill"));
        assert!(!allowed(&allow, "service"));
        assert!(!allowed(&[], "kill"));
    }
}
