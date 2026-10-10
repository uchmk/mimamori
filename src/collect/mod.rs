//! The sampler: one thread that reads the machine through sysinfo on its own
//! schedule (CPU and memory every interval, processes every other) and sends
//! each reading to the UI, waking it. The UI steers it with [`Ctl`]: the
//! interval, pause. Dropping the receiver ends the thread.

pub mod topo;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind, Users};

use topo::Topology;

/// What does not change while running, read once at startup.
#[derive(Clone, Debug)]
pub struct Static {
    pub host: String,
    pub os: String,
    pub kernel: String,
    pub arch: String,
    pub brand: String,
    /// Seconds since the epoch.
    pub boot_time: u64,
    pub topo: Topology,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mem {
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub swap_total: u64,
    pub swap_used: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: Option<u32>,
    pub name: String,
    /// Percent of one logical CPU, top's way: a process busy on two
    /// threads reads 200.
    pub cpu: f32,
    /// Resident bytes.
    pub mem: u64,
    /// `None` where the OS does not say cheaply (macOS for now).
    pub threads: Option<u32>,
    pub user: Option<String>,
    pub cmd: String,
    pub exe: Option<String>,
    /// Seconds since the epoch.
    pub start: u64,
    pub status: String,
}

#[derive(Debug)]
pub enum Msg {
    Static(Box<Static>),
    /// Per logical CPU in sysinfo's order: usage in percent, clock in MHz.
    /// `load` is the 1, 5 and 15 minute load average where the OS keeps one
    /// (not Windows).
    Cpu {
        usage: Vec<f32>,
        freq_mhz: Vec<u64>,
        total: f32,
        load: Option<(f64, f64, f64)>,
    },
    Mem(Mem),
    Procs {
        procs: Vec<Proc>,
        total_threads: Option<u64>,
    },
}

#[derive(Debug)]
pub enum Ctl {
    Interval(Duration),
    Pause(bool),
}

/// Start the sampler; `wake` is repainted after every message.
pub fn spawn(wake: egui::Context, interval: Duration) -> (Receiver<Msg>, Sender<Ctl>) {
    let (tx, rx) = unbounded();
    let (ctl_tx, ctl_rx) = unbounded();
    std::thread::Builder::new()
        .name("mimamori-sampler".into())
        .spawn(move || run(&tx, &ctl_rx, &wake, interval))
        .expect("spawn the sampler thread");
    (rx, ctl_tx)
}

fn run(tx: &Sender<Msg>, ctl: &Receiver<Ctl>, wake: &egui::Context, mut interval: Duration) {
    let send = |m: Msg| {
        let ok = tx.send(m).is_ok();
        wake.request_repaint();
        ok
    };
    let cpu_kind = CpuRefreshKind::nothing().with_cpu_usage().with_frequency();
    let mem_kind = MemoryRefreshKind::nothing().with_ram().with_swap();
    let mut sys = System::new();
    sys.refresh_cpu_specifics(cpu_kind);
    let logical = sys.cpus().len();
    let st = Static {
        host: System::host_name().unwrap_or_default(),
        os: System::long_os_version().unwrap_or_default(),
        kernel: System::kernel_version().unwrap_or_default(),
        arch: System::cpu_arch(),
        brand: sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default(),
        boot_time: System::boot_time(),
        topo: topo::read(logical),
    };
    if !send(Msg::Static(Box::new(st))) {
        return;
    }
    let mut procs = ProcSampler::new();
    let mut paused = false;
    // The first CPU reading needs a gap after the first refresh.
    let mut next = Instant::now() + interval.max(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    let mut tick: u64 = 0;
    loop {
        let wait = next.saturating_duration_since(Instant::now());
        let got = if paused { ctl.recv().map_err(|_| RecvTimeoutError::Disconnected) } else { ctl.recv_timeout(wait) };
        match got {
            Ok(Ctl::Interval(d)) => {
                interval = d;
                next = Instant::now() + interval;
                continue;
            }
            Ok(Ctl::Pause(p)) => {
                paused = p;
                next = Instant::now() + interval;
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
        sys.refresh_cpu_specifics(cpu_kind);
        sys.refresh_memory_specifics(mem_kind);
        let usage: Vec<f32> = sys.cpus().iter().map(|c| c.cpu_usage()).collect();
        let freq_mhz = sys.cpus().iter().map(|c| c.frequency()).collect();
        let total = if usage.is_empty() { 0.0 } else { usage.iter().sum::<f32>() / usage.len() as f32 };
        let mem = Mem {
            total: sys.total_memory(),
            used: sys.used_memory(),
            available: sys.available_memory(),
            swap_total: sys.total_swap(),
            swap_used: sys.used_swap(),
        };
        let load = load_average();
        if !send(Msg::Cpu { usage, freq_mhz, total, load }) || !send(Msg::Mem(mem)) {
            return;
        }
        if tick.is_multiple_of(2) {
            let (procs, total_threads) = procs.sample(&mut sys);
            if !send(Msg::Procs { procs, total_threads }) {
                return;
            }
        }
        tick += 1;
        next += interval;
        if next < Instant::now() {
            // Fell behind (the machine slept): start over from now.
            next = Instant::now() + interval;
        }
    }
}

struct ProcSampler {
    users: Users,
    users_at: Instant,
    names: HashMap<String, String>,
}

impl ProcSampler {
    fn new() -> Self {
        Self { users: Users::new_with_refreshed_list(), users_at: Instant::now(), names: HashMap::new() }
    }

    fn sample(&mut self, sys: &mut System) -> (Vec<Proc>, Option<u64>) {
        let kind = ProcessRefreshKind::nothing()
            .without_tasks()
            .with_cpu()
            .with_memory()
            .with_user(UpdateKind::OnlyIfNotSet)
            .with_cmd(UpdateKind::OnlyIfNotSet)
            .with_exe(UpdateKind::OnlyIfNotSet);
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
        // New accounts are rare; the list is read again at most once a minute.
        if self.users_at.elapsed() > Duration::from_secs(60) {
            self.users.refresh();
            self.names.clear();
            self.users_at = Instant::now();
        }
        let threads = thread_counts();
        let mut out = Vec::with_capacity(sys.processes().len());
        for (pid, p) in sys.processes() {
            if p.thread_kind().is_some() {
                continue;
            }
            let pid = pid.as_u32();
            let user = p.user_id().and_then(|uid| {
                let key = uid.to_string();
                if let Some(n) = self.names.get(&key) {
                    return Some(n.clone());
                }
                let name = self.users.get_user_by_id(uid)?.name().to_string();
                self.names.insert(key, name.clone());
                Some(name)
            });
            let cmd = p.cmd().iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ");
            out.push(Proc {
                pid,
                ppid: p.parent().map(|x| x.as_u32()),
                name: p.name().to_string_lossy().into_owned(),
                cpu: p.cpu_usage(),
                mem: p.memory(),
                threads: threads.as_ref().and_then(|t| t.get(&pid).copied()),
                user,
                cmd,
                exe: p.exe().map(|e| e.display().to_string()),
                start: p.start_time(),
                status: p.status().to_string(),
            });
        }
        let total = threads.map(|t| t.values().map(|&n| n as u64).sum());
        (out, total)
    }
}

#[cfg(not(windows))]
fn load_average() -> Option<(f64, f64, f64)> {
    let l = System::load_average();
    Some((l.one, l.five, l.fifteen))
}

/// Windows keeps no load average.
#[cfg(windows)]
fn load_average() -> Option<(f64, f64, f64)> {
    None
}

/// Threads per process, from `/proc/<pid>/stat`: sysinfo only counts them
/// by listing every task, which costs far more.
#[cfg(target_os = "linux")]
fn thread_counts() -> Option<HashMap<u32, u32>> {
    let mut out = HashMap::new();
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        if let Some(n) = std::fs::read_to_string(entry.path().join("stat")).ok().and_then(|s| stat_threads(&s)) {
            out.insert(pid, n);
        }
    }
    Some(out)
}

/// Field 20 of a stat line. The name in parentheses may hold spaces and
/// parentheses itself, so fields are counted from its last `)`.
#[cfg(any(target_os = "linux", test))]
fn stat_threads(stat: &str) -> Option<u32> {
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(17)?.parse().ok()
}

/// Threads per process from one Toolhelp snapshot.
#[cfg(windows)]
fn thread_counts() -> Option<HashMap<u32, u32>> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };

    // SAFETY: a snapshot of every process, closed below.
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.ok()?;
    let mut out = HashMap::new();
    let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    // SAFETY: `e` is a PROCESSENTRY32W with its size set, as the calls ask.
    let mut ok = unsafe { Process32FirstW(snap, &mut e) }.is_ok();
    while ok {
        out.insert(e.th32ProcessID, e.cntThreads);
        // SAFETY: as above.
        ok = unsafe { Process32NextW(snap, &mut e) }.is_ok();
    }
    // SAFETY: the handle from CreateToolhelp32Snapshot, closed once.
    let _ = unsafe { CloseHandle(snap) };
    Some(out)
}

#[cfg(not(any(target_os = "linux", windows)))]
fn thread_counts() -> Option<HashMap<u32, u32>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threads_come_from_field_20_past_the_name() {
        let stat = "1234 (a (weird) name) S 1 1234 1234 0 -1 4194560 100 0 0 0 5 3 0 0 20 0 7 0 12345 1000 50";
        assert_eq!(stat_threads(stat), Some(7));
        assert_eq!(stat_threads("garbage"), None);
    }
}
