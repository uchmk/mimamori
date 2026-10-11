//! What the procs screen's detail shows of the selected process beyond the
//! list's columns: its priority, the CPUs it may run on, how many files or
//! handles it holds open and how many sockets. Read for that one process on
//! a thread of its own (the sockets walk every process on Linux), so the list
//! keeps its pace.
//!
//! Each OS answers what it can: Linux reads `/proc/<pid>`, Windows opens the
//! process for limited query, macOS asks `getpriority` and `proc_pidinfo`.
//! What the OS refuses (another user's process) stays `None`.

use crossbeam_channel::Sender;

/// How urgent the OS counts a process. Each OS builds only its own kind.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prio {
    /// Unix: -20 (most urgent) to 19.
    Nice(i32),
    /// Windows' priority classes.
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
    Realtime,
}

impl Prio {
    /// The key of the word that names it, `None` for a nice value (a number).
    pub fn key(self) -> Option<&'static str> {
        Some(match self {
            Prio::Nice(_) => return None,
            Prio::Idle => "procs.prio_idle",
            Prio::BelowNormal => "procs.prio_below",
            Prio::Normal => "procs.prio_normal",
            Prio::AboveNormal => "procs.prio_above",
            Prio::High => "procs.prio_high",
            Prio::Realtime => "procs.prio_realtime",
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Info {
    pub priority: Option<Prio>,
    /// The logical CPUs it may run on, as ranges: `0-3,8`.
    pub affinity: Option<String>,
    /// Open files (handles on Windows).
    pub files: Option<u32>,
    /// Sockets it holds, as far as the OS lets this user see.
    pub conns: Option<u32>,
}

/// Read `pid` on a thread of its own; the answer comes back on `tx`.
pub fn spawn(pid: u32, tx: Sender<(u32, Info)>, wake: Option<egui::Context>) -> bool {
    std::thread::Builder::new()
        .name("yagura-procinfo".into())
        .spawn(move || {
            let mut info = read(pid);
            info.conns = conns(pid);
            let _ = tx.send((pid, info));
            if let Some(w) = wake {
                w.request_repaint();
            }
        })
        .is_ok()
}

fn conns(pid: u32) -> Option<u32> {
    let all = crate::collect::net::read_conns();
    if all.list.iter().all(|c| c.pid.is_none()) && !all.list.is_empty() {
        // Nothing says whose they are: a count would read as none.
        return None;
    }
    Some(all.list.iter().filter(|c| c.pid == Some(pid)).count() as u32)
}

/// CPU numbers as ranges: `[0, 1, 2, 3, 8]` is `0-3,8`.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn ranges(cpus: &[usize]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < cpus.len() {
        let mut j = i;
        while j + 1 < cpus.len() && cpus[j + 1] == cpus[j] + 1 {
            j += 1;
        }
        out.push(if j > i { format!("{}-{}", cpus[i], cpus[j]) } else { cpus[i].to_string() });
        i = j + 1;
    }
    out.join(",")
}

/// The nice value in `/proc/<pid>/stat`: the 19th field, counted after the
/// name in parentheses (which may hold spaces and parentheses itself).
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn stat_nice(stat: &str) -> Option<i32> {
    let rest = stat.get(stat.rfind(')')? + 1..)?;
    // After the name: state is field 3, nice field 19.
    rest.split_whitespace().nth(19 - 3)?.parse().ok()
}

/// `Cpus_allowed_list` from `/proc/<pid>/status`.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn status_affinity(status: &str) -> Option<String> {
    let v = status.lines().find_map(|l| l.strip_prefix("Cpus_allowed_list:"))?.trim();
    (!v.is_empty()).then(|| v.replace(", ", ","))
}

#[cfg(target_os = "linux")]
fn read(pid: u32) -> Info {
    let dir = format!("/proc/{pid}");
    Info {
        priority: std::fs::read_to_string(format!("{dir}/stat")).ok().and_then(|s| stat_nice(&s)).map(Prio::Nice),
        affinity: std::fs::read_to_string(format!("{dir}/status")).ok().and_then(|s| status_affinity(&s)),
        files: std::fs::read_dir(format!("{dir}/fd")).ok().map(|d| d.count() as u32),
        conns: None,
    }
}

#[cfg(windows)]
fn read(pid: u32) -> Info {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, GetPriorityClass, GetProcessAffinityMask, GetProcessHandleCount,
        HIGH_PRIORITY_CLASS, IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        REALTIME_PRIORITY_CLASS,
    };
    let mut info = Info::default();
    // SAFETY: the handle is closed before returning; the out values are alive.
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return info };
        info.priority = match GetPriorityClass(h) {
            0 => None,
            c if c == IDLE_PRIORITY_CLASS.0 => Some(Prio::Idle),
            c if c == BELOW_NORMAL_PRIORITY_CLASS.0 => Some(Prio::BelowNormal),
            c if c == NORMAL_PRIORITY_CLASS.0 => Some(Prio::Normal),
            c if c == ABOVE_NORMAL_PRIORITY_CLASS.0 => Some(Prio::AboveNormal),
            c if c == HIGH_PRIORITY_CLASS.0 => Some(Prio::High),
            c if c == REALTIME_PRIORITY_CLASS.0 => Some(Prio::Realtime),
            _ => None,
        };
        let (mut proc_mask, mut sys_mask) = (0usize, 0usize);
        if GetProcessAffinityMask(h, &mut proc_mask, &mut sys_mask).is_ok() && proc_mask != 0 {
            let cpus: Vec<usize> = (0..usize::BITS as usize).filter(|b| proc_mask >> b & 1 == 1).collect();
            info.affinity = Some(ranges(&cpus));
        }
        let mut n = 0u32;
        if GetProcessHandleCount(h, &mut n).is_ok() {
            info.files = Some(n);
        }
        let _ = CloseHandle(h);
    }
    info
}

#[cfg(target_os = "macos")]
fn read(pid: u32) -> Info {
    let mut info = Info::default();
    // SAFETY: plain calls with a buffer of the size we pass; `getpriority`
    // reports an error through errno, which is cleared before.
    unsafe {
        *libc::__error() = 0;
        let nice = libc::getpriority(libc::PRIO_PROCESS, pid as libc::id_t);
        if *libc::__error() == 0 {
            info.priority = Some(Prio::Nice(nice));
        }
        // With no buffer it says how many bytes the list would take.
        let bytes = libc::proc_pidinfo(pid as i32, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0);
        if bytes > 0 {
            info.files = Some(bytes as u32 / size_of::<libc::proc_fdinfo>() as u32);
        }
    }
    info
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn read(_pid: u32) -> Info {
    Info::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpus_become_ranges() {
        assert_eq!(ranges(&[0, 1, 2, 3, 8]), "0-3,8");
        assert_eq!(ranges(&[5]), "5");
        assert_eq!(ranges(&[0, 2, 4]), "0,2,4");
        assert_eq!(ranges(&[]), "");
    }

    #[test]
    fn nice_is_found_after_a_name_with_parentheses() {
        let stat = "123 (a) b (x)) S 1 123 123 0 -1 4194560 100 0 0 0 1 2 0 0 20 5 1 0 100 1000 10 18446744073709551615";
        assert_eq!(stat_nice(stat), Some(5));
        assert_eq!(stat_nice("1 (init) S 0"), None);
    }

    #[test]
    fn affinity_is_read_from_status() {
        assert_eq!(status_affinity("Name:\tx\nCpus_allowed_list:\t0-3, 8\n"), Some("0-3,8".into()));
        assert_eq!(status_affinity("Name:\tx\n"), None);
    }

    #[test]
    fn prio_words() {
        assert_eq!(Prio::Nice(3).key(), None);
        assert_eq!(Prio::High.key(), Some("procs.prio_high"));
    }
}
