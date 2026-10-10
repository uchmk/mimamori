//! Ending a process: the confirmed `x`. Each request runs on its own
//! short-lived thread, since it may wait seconds for the process to go, and
//! reports back through a channel, waking the UI.
//!
//! Windows ends it with `TerminateProcess`; elsewhere it gets SIGTERM and,
//! if it is still there after [`GRACE`], SIGKILL. Before anything is sent the
//! PID is checked against the start time the list showed, so a process that
//! went while the question was up does not take a newcomer with its PID.

use std::time::Duration;

use crossbeam_channel::Sender;
use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};

/// How long a process has after SIGTERM before it gets SIGKILL.
pub const GRACE: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub pid: u32,
    pub name: String,
    /// Seconds since the epoch, as the list showed it; 0 checks nothing.
    pub start: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// It went: after SIGTERM, or after `TerminateProcess` on Windows.
    Ended,
    /// It outlived SIGTERM's grace and SIGKILL ended it. Never on Windows,
    /// which has no SIGTERM to outlive.
    #[cfg_attr(windows, allow(dead_code))]
    Killed,
    /// It had gone, or its PID was another process's, before anything was sent.
    Gone,
    /// The OS refused, in its own words.
    Failed(String),
}

/// End `t` on a thread of its own; the outcome comes back on `tx`.
pub fn spawn(t: Target, tx: Sender<(Target, Outcome)>, wake: Option<egui::Context>) {
    let (t2, tx2) = (t.clone(), tx.clone());
    let started = std::thread::Builder::new().name("mimamori-kill".into()).spawn(move || {
        let o = end(&t, GRACE);
        let _ = tx.send((t, o));
        if let Some(w) = wake {
            w.request_repaint();
        }
    });
    if let Err(e) = started {
        let _ = tx2.send((t2, Outcome::Failed(e.to_string())));
    }
}

/// End `t` and wait for it, `grace` long for SIGTERM. Blocks.
pub fn end(t: &Target, grace: Duration) -> Outcome {
    // 0 would signal our own process group, and a PID past i32 a group too.
    if t.pid == 0 || t.pid > i32::MAX as u32 {
        return Outcome::Gone;
    }
    let mut sys = System::new();
    if !alive(&mut sys, t) {
        return Outcome::Gone;
    }
    os::end(&mut sys, t, grace)
}

/// Whether the process `t` names is still running: not gone, not a zombie
/// waiting for its parent, and not a newer process given the same PID.
fn alive(sys: &mut System, t: &Target) -> bool {
    let pid = Pid::from_u32(t.pid);
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::nothing());
    sys.process(pid).is_some_and(|p| p.status() != ProcessStatus::Zombie && (t.start == 0 || p.start_time().abs_diff(t.start) <= 1))
}

#[cfg(windows)]
mod os {
    use std::time::Duration;

    use sysinfo::System;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject};

    use super::{Outcome, Target, alive};

    pub fn end(sys: &mut System, t: &Target, grace: Duration) -> Outcome {
        // SAFETY: a handle to the process, closed below.
        let h = match unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, t.pid) } {
            Ok(h) => h,
            Err(_) if !alive(sys, t) => return Outcome::Gone,
            Err(e) => return Outcome::Failed(e.message()),
        };
        // SAFETY: the handle opened above with PROCESS_TERMINATE.
        let out = match unsafe { TerminateProcess(h, 1) } {
            Ok(()) => {
                // Termination is queued; give it the grace to finish.
                // SAFETY: the same handle, opened with PROCESS_SYNCHRONIZE.
                let _ = unsafe { WaitForSingleObject(h, grace.as_millis().min(u32::MAX as u128) as u32) };
                Outcome::Ended
            }
            Err(_) if !alive(sys, t) => Outcome::Gone,
            Err(e) => Outcome::Failed(e.message()),
        };
        // SAFETY: the handle from OpenProcess, closed once.
        let _ = unsafe { CloseHandle(h) };
        out
    }
}

#[cfg(not(windows))]
mod os {
    use std::time::{Duration, Instant};

    use sysinfo::{Pid, Signal, System};

    use super::{Outcome, Target, alive};

    const POLL: Duration = Duration::from_millis(100);

    /// Send `sig`; the OS's reason when it refuses.
    fn send(sys: &System, t: &Target, sig: Signal) -> Result<(), String> {
        let Some(p) = sys.process(Pid::from_u32(t.pid)) else { return Ok(()) };
        match p.kill_with(sig) {
            Some(true) => Ok(()),
            // Read straight after the failed kill(2), before anything else
            // can set errno.
            Some(false) => Err(std::io::Error::last_os_error().to_string()),
            None => Err(format!("{sig:?} is not supported here")),
        }
    }

    /// Wait up to `d` for it to go.
    fn gone_within(sys: &mut System, t: &Target, d: Duration) -> bool {
        let until = Instant::now() + d;
        loop {
            if !alive(sys, t) {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(POLL);
        }
    }

    pub fn end(sys: &mut System, t: &Target, grace: Duration) -> Outcome {
        if let Err(e) = send(sys, t, Signal::Term) {
            return if alive(sys, t) { Outcome::Failed(e) } else { Outcome::Gone };
        }
        if gone_within(sys, t, grace) {
            return Outcome::Ended;
        }
        if let Err(e) = send(sys, t, Signal::Kill) {
            return if alive(sys, t) { Outcome::Failed(e) } else { Outcome::Ended };
        }
        // SIGKILL cannot be caught; this only waits for the OS to finish.
        gone_within(sys, t, Duration::from_secs(1));
        Outcome::Killed
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::process::{Child, Command, Stdio};

    use super::*;

    /// A child that sleeps for half a minute.
    pub(crate) fn sleeper() -> Child {
        let mut c = if cfg!(windows) {
            let mut c = Command::new("ping");
            c.args(["-n", "30", "127.0.0.1"]);
            c
        } else {
            let mut c = Command::new("sleep");
            c.arg("30");
            c
        };
        c.stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("start a child to end")
    }

    fn target(c: &Child) -> Target {
        Target { pid: c.id(), name: "child".into(), start: 0 }
    }

    #[test]
    fn a_running_child_is_ended() {
        let mut c = sleeper();
        assert_eq!(end(&target(&c), Duration::from_secs(5)), Outcome::Ended);
        assert!(!c.wait().unwrap().success());
    }

    #[test]
    fn a_pid_that_is_not_running_is_gone_and_nothing_is_sent() {
        let t = Target { pid: 9_999_999, name: "nobody".into(), start: 0 };
        assert_eq!(end(&t, Duration::from_secs(1)), Outcome::Gone);
        assert_eq!(end(&Target { pid: 0, ..t.clone() }, Duration::from_secs(1)), Outcome::Gone);
    }

    #[test]
    fn a_pid_now_held_by_another_process_is_left_alone() {
        let mut c = sleeper();
        // The list's start time is a day off: the PID is someone else's now.
        let t = Target { start: 86_400, ..target(&c) };
        assert_eq!(end(&t, Duration::from_secs(1)), Outcome::Gone);
        assert!(c.try_wait().unwrap().is_none(), "the child must still be running");
        let _ = c.kill();
        let _ = c.wait();
    }

    #[cfg(unix)]
    #[test]
    fn a_process_that_ignores_sigterm_gets_sigkill() {
        // The shell says when its trap is set, so the signal cannot come first.
        let mut c = Command::new("sh")
            .args(["-c", "trap '' TERM; echo ready; while :; do sleep 1; done"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut line = String::new();
        std::io::BufRead::read_line(&mut std::io::BufReader::new(c.stdout.take().unwrap()), &mut line).unwrap();
        assert_eq!(line.trim(), "ready");
        assert_eq!(end(&target(&c), Duration::from_millis(300)), Outcome::Killed);
        assert!(!c.wait().unwrap().success());
    }
}
