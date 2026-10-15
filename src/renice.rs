//! Changing the selected process's priority one step: the unconfirmed `}` and
//! `{` in procs. Each request runs on its own short-lived thread (the OS may
//! be slow to answer) and reports back through a channel, waking the UI.
//!
//! Windows moves between its priority classes from Idle to High (Realtime is
//! never entered by a key: it can starve the system). Elsewhere the nice value
//! moves by [`NICE_STEP`] within -20..19, on every thread of the process on
//! Linux, where `setpriority` takes one thread at a time. What the OS refuses
//! (raising without privilege, another user's process) is told in its words.

use crossbeam_channel::Sender;

use crate::kill::Target;
use crate::procinfo::Prio;

/// How far one press moves a nice value.
pub const NICE_STEP: i32 = 5;

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// It is now this.
    Set(Prio),
    /// Already as urgent or as idle as a key goes.
    Limit(Prio),
    /// It had gone, or its PID was another process's.
    Gone,
    /// The priority could not be read: nothing to step from.
    Unknown,
    /// The OS refused, in its own words.
    Failed(String),
}

/// The priority one step more urgent (`raise`) or less, `None` at the end of
/// what a key reaches.
pub fn step(cur: Prio, raise: bool) -> Option<Prio> {
    const CLASSES: [Prio; 5] = [Prio::Idle, Prio::BelowNormal, Prio::Normal, Prio::AboveNormal, Prio::High];
    match cur {
        Prio::Nice(n) => {
            let to = if raise { n - NICE_STEP } else { n + NICE_STEP }.clamp(-20, 19);
            (to != n).then_some(Prio::Nice(to))
        }
        // Realtime steps down to High; going up from it is nothing.
        Prio::Realtime => (!raise).then_some(Prio::High),
        c => {
            let at = CLASSES.iter().position(|x| *x == c)?;
            let to = if raise { at + 1 } else { at.checked_sub(1)? };
            CLASSES.get(to).copied()
        }
    }
}

/// Step `t` on a thread of its own; the outcome comes back on `tx`.
pub fn spawn(t: Target, raise: bool, tx: Sender<(Target, Outcome)>, wake: Option<egui::Context>) {
    let (t2, tx2) = (t.clone(), tx.clone());
    let started = std::thread::Builder::new().name("yagura-renice".into()).spawn(move || {
        let o = change(&t, raise);
        let _ = tx.send((t, o));
        if let Some(w) = wake {
            w.request_repaint();
        }
    });
    if let Err(e) = started {
        let _ = tx2.send((t2, Outcome::Failed(e.to_string())));
    }
}

/// Step `t`'s priority. Blocks.
pub fn change(t: &Target, raise: bool) -> Outcome {
    if t.pid == 0 || t.pid > i32::MAX as u32 {
        return Outcome::Gone;
    }
    if !crate::kill::is_alive(t) {
        return Outcome::Gone;
    }
    let Some(cur) = crate::procinfo::read_priority(t.pid) else { return Outcome::Unknown };
    let Some(to) = step(cur, raise) else { return Outcome::Limit(cur) };
    match os::set(t.pid, to) {
        Ok(()) => Outcome::Set(to),
        Err(_) if !crate::kill::is_alive(t) => Outcome::Gone,
        Err(e) => Outcome::Failed(e),
    }
}

#[cfg(windows)]
mod os {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS, IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
        OpenProcess, PROCESS_SET_INFORMATION, REALTIME_PRIORITY_CLASS, SetPriorityClass,
    };

    use crate::procinfo::Prio;

    pub fn set(pid: u32, to: Prio) -> Result<(), String> {
        let class = match to {
            Prio::Idle => IDLE_PRIORITY_CLASS,
            Prio::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
            Prio::Normal | Prio::Nice(_) => NORMAL_PRIORITY_CLASS,
            Prio::AboveNormal => ABOVE_NORMAL_PRIORITY_CLASS,
            Prio::High => HIGH_PRIORITY_CLASS,
            Prio::Realtime => REALTIME_PRIORITY_CLASS,
        };
        // SAFETY: a handle to the process, closed below.
        let h = unsafe { OpenProcess(PROCESS_SET_INFORMATION, false, pid) }.map_err(|e| e.message())?;
        // SAFETY: the handle opened above with PROCESS_SET_INFORMATION.
        let r = unsafe { SetPriorityClass(h, class) }.map_err(|e| e.message());
        // SAFETY: the handle from OpenProcess, closed once.
        let _ = unsafe { CloseHandle(h) };
        r
    }
}

#[cfg(unix)]
mod os {
    use crate::procinfo::Prio;

    /// The threads of `pid`: Linux's `setpriority` moves one at a time.
    #[cfg(target_os = "linux")]
    fn tids(pid: u32) -> Vec<u32> {
        let all: Vec<u32> = std::fs::read_dir(format!("/proc/{pid}/task"))
            .map(|d| d.flatten().filter_map(|e| e.file_name().to_str()?.parse().ok()).collect())
            .unwrap_or_default();
        if all.is_empty() { vec![pid] } else { all }
    }

    #[cfg(not(target_os = "linux"))]
    fn tids(pid: u32) -> Vec<u32> {
        vec![pid]
    }

    pub fn set(pid: u32, to: Prio) -> Result<(), String> {
        let Prio::Nice(n) = to else { return Err("not a nice value".into()) };
        let mut first_err = None;
        for tid in tids(pid) {
            // SAFETY: a plain call; errno is read straight after a failure.
            let r = unsafe { libc::setpriority(libc::PRIO_PROCESS, tid as libc::id_t, n) };
            if r != 0 {
                first_err.get_or_insert_with(|| std::io::Error::last_os_error().to_string());
            }
        }
        first_err.map_or(Ok(()), Err)
    }
}

#[cfg(not(any(windows, unix)))]
mod os {
    use crate::procinfo::Prio;

    pub fn set(_pid: u32, _to: Prio) -> Result<(), String> {
        Err("not supported here".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_moves_in_steps_and_stops_at_the_ends() {
        assert_eq!(step(Prio::Nice(0), true), Some(Prio::Nice(-5)));
        assert_eq!(step(Prio::Nice(0), false), Some(Prio::Nice(5)));
        assert_eq!(step(Prio::Nice(17), false), Some(Prio::Nice(19)));
        assert_eq!(step(Prio::Nice(19), false), None);
        assert_eq!(step(Prio::Nice(-18), true), Some(Prio::Nice(-20)));
        assert_eq!(step(Prio::Nice(-20), true), None);
    }

    #[test]
    fn classes_step_between_idle_and_high() {
        assert_eq!(step(Prio::Normal, true), Some(Prio::AboveNormal));
        assert_eq!(step(Prio::Normal, false), Some(Prio::BelowNormal));
        assert_eq!(step(Prio::Idle, false), None);
        assert_eq!(step(Prio::High, true), None);
        assert_eq!(step(Prio::Realtime, true), None);
        assert_eq!(step(Prio::Realtime, false), Some(Prio::High));
    }
}
