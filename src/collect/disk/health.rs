//! SMART health, asked once a minute (and when the drives change) in a thread
//! of its own: `smartctl` takes a moment per drive and an IOCTL can wait on a
//! busy one, so the collect thread never waits for it. Each tick takes the
//! last answer; a drive not answered yet reads as pending.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, TryRecvError};

use super::{Drive, Health, Temp, Why};

const EVERY: Duration = Duration::from_secs(60);

/// A drive to ask about.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(not(any(windows, target_os = "linux", target_os = "macos")), allow(dead_code))]
pub struct Ask {
    pub id: String,
    pub bus: Option<&'static str>,
}

/// What one drive answered.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Check {
    pub health: Option<Health>,
    pub why: Option<Why>,
    /// The temperature the check read on the way (`smartctl`), for a drive
    /// that has none from the reading itself.
    pub temp: Option<Temp>,
}

impl Check {
    pub fn why(why: Why) -> Self {
        Self { why: Some(why), ..Self::default() }
    }
}

#[derive(Default)]
pub struct Checker {
    /// The check under way.
    rx: Option<Receiver<HashMap<String, Check>>>,
    next: Option<Instant>,
    /// The drives the last check was started for, sorted.
    asked: Vec<String>,
    got: HashMap<String, Check>,
}

impl Checker {
    /// Take a finished check, start the next one when it is due, and fill in
    /// the drives from the last answers.
    pub fn apply(&mut self, drives: &mut [Drive]) {
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(got) => (self.got, self.rx) = (got, None),
                Err(TryRecvError::Disconnected) => self.rx = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        let mut ids: Vec<String> = drives.iter().map(|d| d.id.clone()).collect();
        ids.sort();
        let due = self.next.is_none_or(|t| Instant::now() >= t) || ids != self.asked;
        if due && self.rx.is_none() && !drives.is_empty() {
            let asks = drives.iter().map(|d| Ask { id: d.id.clone(), bus: d.bus }).collect();
            let (tx, rx) = crossbeam_channel::bounded(1);
            let spawned = std::thread::Builder::new().name("disk-health".into()).spawn(move || {
                let _ = tx.send(check(asks));
            });
            if spawned.is_ok() {
                self.rx = Some(rx);
            }
            self.asked = ids;
            self.next = Some(Instant::now() + EVERY);
        }
        fill(drives, &self.got);
    }
}

fn fill(drives: &mut [Drive], got: &HashMap<String, Check>) {
    for d in drives {
        match got.get(&d.id) {
            Some(c) => {
                (d.health, d.why) = (c.health, c.why);
                if d.temp.is_none() {
                    d.temp = c.temp.clone();
                }
            }
            None => d.why = Some(Why::Pending),
        }
    }
}

#[cfg(windows)]
fn check(asks: Vec<Ask>) -> HashMap<String, Check> {
    super::ioctl::check(&asks)
}

#[cfg(target_os = "linux")]
fn check(asks: Vec<Ask>) -> HashMap<String, Check> {
    super::linux::smart(&asks)
}

#[cfg(target_os = "macos")]
fn check(asks: Vec<Ask>) -> HashMap<String, Check> {
    super::mac::check(&asks)
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn check(asks: Vec<Ask>) -> HashMap<String, Check> {
    asks.into_iter().map(|a| (a.id, Check::why(Why::NotRead))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(id: &str) -> Drive {
        Drive { id: id.into(), ..Drive::default() }
    }

    #[test]
    fn answers_fill_the_drives_and_the_rest_are_pending() {
        let t = |v: f32| Temp { value: v, source: "smartctl".into(), ..Temp::default() };
        let got = HashMap::from([
            ("sda".to_string(), Check { health: Some(Health::Good), why: None, temp: Some(t(35.0)) }),
            ("sdb".to_string(), Check { health: Some(Health::Failing), why: None, temp: Some(t(50.0)) }),
            ("vda".to_string(), Check::why(Why::Unsupported)),
        ]);
        // sdb has a temperature of its own (hwmon), which stays.
        let mut drives = vec![drive("sda"), Drive { temp: Some(t(41.0)), ..drive("sdb") }, drive("vda"), drive("sdc")];
        fill(&mut drives, &got);
        assert_eq!((drives[0].health, drives[0].why, drives[0].temp.as_ref().map(|t| t.value)), (Some(Health::Good), None, Some(35.0)));
        assert_eq!((drives[1].health, drives[1].temp.as_ref().map(|t| t.value)), (Some(Health::Failing), Some(41.0)));
        assert_eq!((drives[2].health, drives[2].why), (None, Some(Why::Unsupported)));
        assert_eq!((drives[3].health, drives[3].why), (None, Some(Why::Pending)));
    }

    #[test]
    fn a_check_starts_at_once_and_again_when_the_drives_change() {
        let mut c = Checker::default();
        c.apply(&mut []);
        assert!(c.rx.is_none(), "no drives, nothing to ask");
        let mut drives = vec![drive("b"), drive("a")];
        c.apply(&mut drives);
        assert_eq!(c.asked, ["a", "b"]);
        assert_eq!(drives[0].why, Some(Why::Pending));
        // Wait for the answer; on this OS it may be anything but pending.
        let got = c.rx.take().unwrap().recv_timeout(Duration::from_secs(30)).unwrap();
        assert_eq!(got.len(), 2);
        c.got = got;
        c.apply(&mut drives);
        assert!(c.rx.is_none(), "not due again for a minute");
        assert_ne!(drives[0].why, Some(Why::Pending));
        let mut more = vec![drive("a"), drive("b"), drive("c")];
        c.apply(&mut more);
        assert_eq!(c.asked, ["a", "b", "c"]);
        assert!(c.rx.is_some());
    }
}
