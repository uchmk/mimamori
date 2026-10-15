//! Choosing the CPUs the selected process may run on: `A` in procs opens an
//! editor where they are typed as ranges (`0-3,8`) or clicked. What it may
//! run on now and which CPUs there are to choose from are read on a thread
//! of their own ([`look_spawn`]), and so is the change, as [`crate::renice`]'s
//! is; the CPUs are read back so the toast tells what the OS made of it.
//!
//! The CPUs are numbered as the OS numbers them, which may leave gaps (Linux,
//! with some offline). Windows sets the process's mask
//! (`SetProcessAffinityMask`), within the processor group the process is in,
//! so at most 64 CPUs. Linux sets every thread of the process, since
//! `sched_setaffinity` takes one thread at a time. macOS has no way to pin a
//! process, so the key only says so there.

use crossbeam_channel::Sender;

use crate::i18n::trf;
use crate::kill::Target;
use crate::procinfo::ranges;

/// Whether this OS can choose a process's CPUs.
pub const SUPPORTED: bool = cfg!(any(windows, target_os = "linux"));

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// It may now run on these CPUs, as ranges.
    Set(String),
    /// It had gone, or its PID was another process's.
    Gone,
    /// The OS refused, in its own words.
    Failed(String),
}

/// What the editor starts from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Look {
    /// The CPUs it may run on now, as ranges; none when the OS would not say.
    pub now: Option<String>,
    /// The CPUs there are to choose from, sorted, as the OS numbers them.
    pub ids: Vec<usize>,
}

/// What is wrong with CPUs as typed.
#[derive(Clone, Debug, PartialEq)]
pub enum Bad {
    /// Nothing chosen: a process must run somewhere.
    Empty,
    /// A piece that is not a number or a range.
    Word(String),
    /// A range that goes down: `3-1`.
    Backwards(String),
    /// A CPU there is not, and the ones there are, as ranges.
    Beyond(usize, String),
}

impl Bad {
    pub fn message(&self) -> String {
        match self {
            Bad::Empty => crate::i18n::tr("aff.empty").to_string(),
            Bad::Word(w) => trf("aff.word", &[w]),
            Bad::Backwards(w) => trf("aff.backwards", &[w]),
            Bad::Beyond(c, there) => trf("aff.beyond", &[&c.to_string(), there]),
        }
    }
}

/// One piece between commas as two ends: `5` is `(5, 5)`.
fn ends(p: &str) -> Result<(usize, usize), Bad> {
    let num = |s: &str| s.trim().parse::<usize>().map_err(|_| Bad::Word(p.to_string()));
    let (a, b) = match p.split_once('-') {
        Some((a, b)) => (num(a)?, num(b)?),
        None => {
            let a = num(p)?;
            (a, a)
        }
    };
    if a > b { Err(Bad::Backwards(p.to_string())) } else { Ok((a, b)) }
}

/// The CPUs of `ids` from `a` to `b`.
fn within(ids: &[usize], a: usize, b: usize) -> impl Iterator<Item = usize> + '_ {
    ids.iter().copied().filter(move |&c| a <= c && c <= b)
}

fn pieces(text: &str) -> impl Iterator<Item = &str> {
    text.split(',').map(str::trim).filter(|p| !p.is_empty())
}

/// CPUs typed as `0-3, 8`, out of `ids`: sorted, each once. A CPU named on
/// its own must be one of them; a range takes those in it (a gap is not a
/// fault), but may not end past the last.
pub fn parse(text: &str, ids: &[usize]) -> Result<Vec<usize>, Bad> {
    let mut cpus = Vec::new();
    for p in pieces(text) {
        let (a, b) = ends(p)?;
        let at = cpus.len();
        cpus.extend(within(ids, a, b));
        let past = ids.last().is_none_or(|&l| b > l);
        if past || cpus.len() == at {
            return Err(Bad::Beyond(if a == b || !past { a } else { b }, ranges(ids)));
        }
    }
    cpus.sort_unstable();
    cpus.dedup();
    if cpus.is_empty() { Err(Bad::Empty) } else { Ok(cpus) }
}

/// The CPUs of `ids` in the pieces that read, skipping the rest: what the
/// boxes show while the line is half typed, what a click starts from, and
/// what is kept of a reading that names CPUs there are not.
pub fn lenient(text: &str, ids: &[usize]) -> Vec<usize> {
    let mut cpus: Vec<usize> = pieces(text).filter_map(|p| ends(p).ok()).flat_map(|(a, b)| within(ids, a, b)).collect();
    cpus.sort_unstable();
    cpus.dedup();
    cpus
}

/// Read what `pid` may run on and which CPUs there are, on a thread of its
/// own; the answer comes back on `tx`.
pub fn look_spawn(pid: u32, tx: Sender<(u32, Look)>, wake: Option<egui::Context>) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("yagura-affinity-look".into())
        .spawn(move || {
            let _ = tx.send((pid, look(pid)));
            if let Some(w) = wake {
                w.request_repaint();
            }
        })
        .map(drop)
}

/// What `pid` may run on and which CPUs there are. Blocks.
pub fn look(pid: u32) -> Look {
    let mut l = os::look(pid);
    if l.ids.is_empty() {
        let n = std::thread::available_parallelism().map_or(1, |n| n.get());
        l.ids = (0..n).collect();
    }
    l
}

/// Set `t`'s CPUs on a thread of its own; the outcome comes back on `tx`.
pub fn spawn(t: Target, cpus: Vec<usize>, tx: Sender<(Target, Outcome)>, wake: Option<egui::Context>) {
    let (t2, tx2) = (t.clone(), tx.clone());
    let started = std::thread::Builder::new().name("yagura-affinity".into()).spawn(move || {
        let o = change(&t, &cpus);
        let _ = tx.send((t, o));
        if let Some(w) = wake {
            w.request_repaint();
        }
    });
    if let Err(e) = started {
        let _ = tx2.send((t2, Outcome::Failed(e.to_string())));
    }
}

/// Let `t` run on `cpus` only. Blocks.
pub fn change(t: &Target, cpus: &[usize]) -> Outcome {
    if t.pid == 0 || t.pid > i32::MAX as u32 {
        return Outcome::Gone;
    }
    if !crate::kill::is_alive(t) {
        return Outcome::Gone;
    }
    match os::set(t.pid, cpus) {
        Ok(()) => Outcome::Set(crate::procinfo::read_affinity(t.pid).unwrap_or_else(|| ranges(cpus))),
        Err(_) if !crate::kill::is_alive(t) => Outcome::Gone,
        Err(e) => Outcome::Failed(e),
    }
}

/// An OS error in its words, without Rust's `(os error N)` after them.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn said(e: &std::io::Error) -> String {
    let s = e.to_string();
    match s.rfind(" (os error ") {
        Some(at) if s.ends_with(')') => s[..at].to_string(),
        _ => s,
    }
}

#[cfg(windows)]
mod os {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetProcessAffinityMask, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
        SetProcessAffinityMask,
    };

    use super::Look;

    fn bits(mask: usize) -> Vec<usize> {
        (0..usize::BITS as usize).filter(|b| mask >> b & 1 == 1).collect()
    }

    /// The process's mask, and the CPUs of its processor group. A process
    /// with threads in more than one group has neither; the CPUs are then
    /// yagura's own group's.
    pub fn look(pid: u32) -> Look {
        let mut l = Look::default();
        let (mut mine, mut all) = (0usize, 0usize);
        // SAFETY: the handle is closed before leaving; the out values are alive.
        unsafe {
            if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                if GetProcessAffinityMask(h, &mut mine, &mut all).is_ok() && all != 0 {
                    l.ids = bits(all);
                    if mine != 0 {
                        l.now = Some(super::ranges(&bits(mine)));
                    }
                }
                let _ = CloseHandle(h);
            }
            if l.ids.is_empty() && GetProcessAffinityMask(GetCurrentProcess(), &mut mine, &mut all).is_ok() {
                l.ids = bits(all);
            }
        }
        l
    }

    pub fn set(pid: u32, cpus: &[usize]) -> Result<(), String> {
        if let Some(c) = cpus.iter().find(|&&c| c >= usize::BITS as usize) {
            return Err(format!("CPU {c} is in another processor group"));
        }
        let mask = cpus.iter().fold(0usize, |m, &c| m | 1 << c);
        // SAFETY: a handle to the process, closed below.
        let h = unsafe { OpenProcess(PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.map_err(|e| e.message())?;
        // SAFETY: the handle opened above with PROCESS_SET_INFORMATION.
        let r = unsafe { SetProcessAffinityMask(h, mask) }.map_err(|e| e.message());
        // SAFETY: the handle from OpenProcess, closed once.
        let _ = unsafe { CloseHandle(h) };
        r
    }
}

#[cfg(target_os = "linux")]
mod os {
    use std::collections::HashSet;

    use super::Look;

    /// The threads of `pid`: `sched_setaffinity` moves one at a time.
    fn tids(pid: u32) -> Vec<u32> {
        let all: Vec<u32> = std::fs::read_dir(format!("/proc/{pid}/task"))
            .map(|d| d.flatten().filter_map(|e| e.file_name().to_str()?.parse().ok()).collect())
            .unwrap_or_default();
        if all.is_empty() { vec![pid] } else { all }
    }

    /// `Cpus_allowed_list`, and the CPUs online (`0-3,8-11`; a gap is one
    /// switched off).
    pub fn look(pid: u32) -> Look {
        let now = std::fs::read_to_string(format!("/proc/{pid}/status")).ok().and_then(|s| crate::procinfo::status_affinity(&s));
        let ids = std::fs::read_to_string("/sys/devices/system/cpu/online")
            .map(|s| super::lenient(s.trim(), &(0..libc::CPU_SETSIZE as usize).collect::<Vec<_>>()))
            .unwrap_or_default();
        Look { now, ids }
    }

    pub fn set(pid: u32, cpus: &[usize]) -> Result<(), String> {
        if let Some(c) = cpus.iter().find(|&&c| c >= libc::CPU_SETSIZE as usize) {
            return Err(format!("CPU {c} is past what one call can set"));
        }
        // SAFETY: a zeroed cpu_set_t is an empty set; CPU_SET stays within
        // CPU_SETSIZE, checked above.
        let set = unsafe {
            let mut set: libc::cpu_set_t = std::mem::zeroed();
            libc::CPU_ZERO(&mut set);
            for &c in cpus {
                libc::CPU_SET(c, &mut set);
            }
            set
        };
        // Threads started while the others were being set keep the old
        // CPUs, so the list is read again until it brings none new.
        let (mut seen, mut done, mut failed) = (HashSet::new(), 0usize, 0usize);
        let mut first = None;
        for _ in 0..3 {
            let fresh: Vec<u32> = tids(pid).into_iter().filter(|t| seen.insert(*t)).collect();
            if fresh.is_empty() {
                break;
            }
            for tid in fresh {
                // SAFETY: a plain call with a set of the size passed; errno
                // is read straight after a failure.
                let r = unsafe { libc::sched_setaffinity(tid as libc::pid_t, size_of::<libc::cpu_set_t>(), &set) };
                if r == 0 {
                    done += 1;
                    continue;
                }
                let e = std::io::Error::last_os_error();
                // A thread that ended meanwhile needs nothing.
                if e.raw_os_error() == Some(libc::ESRCH) {
                    continue;
                }
                failed += 1;
                if first.is_none() {
                    first = Some(e);
                }
            }
        }
        let Some(e) = first else { return Ok(()) };
        let mut s = if e.raw_os_error() == Some(libc::EINVAL) {
            "none of these CPUs is open to it (offline, or outside its cpuset)".to_string()
        } else {
            super::said(&e)
        };
        if done > 0 {
            s = format!("{s} ({failed} of {} threads)", done + failed);
        }
        Err(s)
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod os {
    use super::Look;

    pub fn look(_pid: u32) -> Look {
        Look::default()
    }

    pub fn set(_pid: u32, _cpus: &[usize]) -> Result<(), String> {
        Err("not supported here".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upto(n: usize) -> Vec<usize> {
        (0..n).collect()
    }

    #[test]
    fn cpus_are_read_as_ranges_and_numbers() {
        assert_eq!(parse("0-3,8", &upto(16)), Ok(vec![0, 1, 2, 3, 8]));
        assert_eq!(parse(" 2 , 0 - 1 ,, 1 ", &upto(4)), Ok(vec![0, 1, 2]));
        assert_eq!(parse("5", &upto(8)), Ok(vec![5]));
    }

    #[test]
    fn what_cannot_be_set_is_told() {
        let four = upto(4);
        assert_eq!(parse("", &four), Err(Bad::Empty));
        assert_eq!(parse(" , ", &four), Err(Bad::Empty));
        assert_eq!(parse("0,x", &four), Err(Bad::Word("x".into())));
        assert_eq!(parse("1-", &four), Err(Bad::Word("1-".into())));
        assert_eq!(parse("3-1", &four), Err(Bad::Backwards("3-1".into())));
        assert_eq!(parse("0-4", &four), Err(Bad::Beyond(4, "0-3".into())));
        assert_eq!(parse("9", &four), Err(Bad::Beyond(9, "0-3".into())));
        // A range as wide as a number goes is told, not walked.
        assert_eq!(parse("0-18446744073709551615", &four), Err(Bad::Beyond(usize::MAX, "0-3".into())));
    }

    /// Linux numbers CPUs as they were found, with gaps where some are off.
    #[test]
    fn cpus_with_gaps_are_the_ones_there_are() {
        let ids = vec![0, 1, 2, 3, 8, 9, 10, 11];
        assert_eq!(parse("8-11", &ids), Ok(vec![8, 9, 10, 11]));
        assert_eq!(parse("0-11", &ids), Ok(ids.clone()));
        assert_eq!(parse("5", &ids), Err(Bad::Beyond(5, "0-3,8-11".into())));
        assert_eq!(parse("4-7", &ids), Err(Bad::Beyond(4, "0-3,8-11".into())));
        assert_eq!(parse("8-12", &ids), Err(Bad::Beyond(12, "0-3,8-11".into())));
        assert_eq!(lenient("8-11", &ids), vec![8, 9, 10, 11]);
        // A reading wider than the CPUs there are keeps the ones there are.
        assert_eq!(lenient("0-63", &ids), ids);
    }

    #[test]
    fn a_half_typed_line_shows_what_reads() {
        assert_eq!(lenient("0-1,x,3-", &upto(4)), vec![0, 1]);
        assert_eq!(lenient("3,2,9", &upto(4)), vec![2, 3]);
        assert_eq!(lenient("", &upto(4)), Vec::<usize>::new());
    }

    #[test]
    fn messages_name_the_piece() {
        assert_eq!(Bad::Word("x".into()).message(), trf("aff.word", &["x"]));
        assert_eq!(Bad::Beyond(9, "0-3,8".into()).message(), trf("aff.beyond", &["9", "0-3,8"]));
    }

    #[test]
    fn an_os_error_loses_its_number() {
        assert!(!said(&std::io::Error::from_raw_os_error(2)).contains("os error"));
        assert_eq!(said(&std::io::Error::other("plain")), "plain");
    }

    #[test]
    fn nothing_is_set_on_a_pid_that_cannot_be_a_process() {
        let t = Target { pid: 0, name: "x".into(), start: 0 };
        assert_eq!(change(&t, &[0]), Outcome::Gone);
        let t = Target { pid: u32::MAX, name: "x".into(), start: 0 };
        assert_eq!(change(&t, &[0]), Outcome::Gone);
    }

    /// This process may run somewhere, on CPUs there are.
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn this_process_is_looked_at() {
        let l = look(std::process::id());
        assert!(!l.ids.is_empty());
        let now = l.now.expect("this process's CPUs");
        assert!(!lenient(&now, &l.ids).is_empty(), "{now} within {:?}", l.ids);
    }

    /// The first CPU this test may run on: a child may be pinned there.
    #[cfg(any(windows, target_os = "linux"))]
    fn first_cpu() -> usize {
        let l = look(std::process::id());
        l.now.and_then(|n| lenient(&n, &l.ids).first().copied()).unwrap_or(0)
    }

    /// A child of the test pinned to one CPU and read back.
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn a_child_is_pinned_to_one_cpu() {
        #[cfg(windows)]
        let mut child =
            std::process::Command::new("ping").args(["-n", "30", "127.0.0.1"]).stdout(std::process::Stdio::null()).spawn().unwrap();
        #[cfg(not(windows))]
        let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let cpu = first_cpu();
        let t = Target { pid: child.id(), name: "child".into(), start: 0 };
        let o = change(&t, &[cpu]);
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(o, Outcome::Set(cpu.to_string()));
    }
}
