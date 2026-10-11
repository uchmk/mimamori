//! What the screens draw from: the latest readings and the last minute of
//! history, folded in from the sampler's messages.

use std::collections::{HashMap, VecDeque};

use crate::collect::{Mem, Msg, Proc, Static, battery, containers, devices, disk, gpu, hw, net, sensors, svc};

/// Samples kept for the graphs and the heatmap: a minute at the default
/// interval.
pub const HISTORY: usize = 60;

/// Samples of the long history's finest tier: five minutes at the default
/// interval.
const RAW: usize = 300;
/// The middle tier is the mean of this many samples, `MID` of them: fifteen
/// minutes.
const MID_EVERY: usize = 5;
const MID: usize = 180;
/// The coarsest tier: the mean of this many samples, `SLOW` of them: an hour.
const SLOW_EVERY: usize = 30;
const SLOW: usize = 120;

/// How far back a zoomed graph looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Range {
    #[default]
    Min5,
    Min15,
    Hour1,
}

impl Range {
    /// The next longer (+1) or shorter (-1) range; the ends stay.
    pub fn step(self, by: i32) -> Self {
        let all = [Self::Min5, Self::Min15, Self::Hour1];
        let i = all.iter().position(|&r| r == self).unwrap_or(0) as i32;
        all[(i + by).clamp(0, 2) as usize]
    }

    /// Samples the range holds when full.
    pub fn slots(self) -> usize {
        match self {
            Self::Min5 => RAW,
            Self::Min15 => MID,
            Self::Hour1 => SLOW,
        }
    }

    /// Samples one point of the range stands for.
    pub fn per_point(self) -> usize {
        match self {
            Self::Min5 => 1,
            Self::Min15 => MID_EVERY,
            Self::Hour1 => SLOW_EVERY,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Min5 => "zoom.min5",
            Self::Min15 => "zoom.min15",
            Self::Hour1 => "zoom.hour1",
        }
    }
}

/// The long history of a system-level series: every sample for five minutes,
/// then means of coarser steps for fifteen minutes and an hour.
#[derive(Clone, Debug, Default)]
struct Long {
    raw: VecDeque<f32>,
    mid: VecDeque<f32>,
    slow: VecDeque<f32>,
    count: usize,
    sum_mid: f32,
    sum_slow: f32,
}

fn keep(q: &mut VecDeque<f32>, cap: usize, v: f32) {
    if q.len() == cap {
        q.pop_front();
    }
    q.push_back(v);
}

impl Long {
    fn push(&mut self, v: f32) {
        keep(&mut self.raw, RAW, v);
        self.count += 1;
        self.sum_mid += v;
        self.sum_slow += v;
        if self.count.is_multiple_of(MID_EVERY) {
            keep(&mut self.mid, MID, self.sum_mid / MID_EVERY as f32);
            self.sum_mid = 0.0;
        }
        if self.count.is_multiple_of(SLOW_EVERY) {
            keep(&mut self.slow, SLOW, self.sum_slow / SLOW_EVERY as f32);
            self.sum_slow = 0.0;
        }
    }
}

/// The newest [`HISTORY`] values, oldest first; a [`Ring::long`] one also
/// keeps the longer tiers for the zoomed graph.
#[derive(Clone, Debug, Default)]
pub struct Ring {
    recent: VecDeque<f32>,
    long: Option<Box<Long>>,
}

impl Ring {
    /// A ring that also keeps the last hour, for the graphs that can be zoomed.
    pub fn long() -> Self {
        Self { recent: VecDeque::new(), long: Some(Box::default()) }
    }

    /// Starts keeping the long tiers if it does not yet (for a ring made by
    /// `Default`, as a field of the model is).
    fn grow(&mut self) -> &mut Self {
        self.long.get_or_insert_with(Box::default);
        self
    }

    pub fn push(&mut self, v: f32) {
        keep(&mut self.recent, HISTORY, v);
        if let Some(l) = &mut self.long {
            l.push(v);
        }
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = f32> + '_ {
        self.recent.iter().copied()
    }

    /// The values of `range`, oldest first (the last minute when the ring
    /// keeps nothing longer).
    pub fn span(&self, range: Range) -> Vec<f32> {
        match (&self.long, range) {
            (None, _) => self.iter().collect(),
            (Some(l), Range::Min5) => l.raw.iter().copied().collect(),
            (Some(l), Range::Min15) => l.mid.iter().copied().collect(),
            (Some(l), Range::Hour1) => l.slow.iter().copied().collect(),
        }
    }

    pub fn last(&self) -> Option<f32> {
        self.recent.back().copied()
    }

    pub fn max(&self) -> f32 {
        self.recent.iter().copied().fold(0.0, f32::max)
    }

    /// The least value, 0 when empty.
    pub fn min(&self) -> f32 {
        if self.recent.is_empty() { 0.0 } else { self.recent.iter().copied().fold(f32::INFINITY, f32::min) }
    }

    pub fn len(&self) -> usize {
        self.recent.len()
    }
}

/// Which history a zoomed graph shows, found again in the model each frame
/// so the graph goes on moving.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Series {
    Cpu,
    Mem,
    Swap,
    GpuUsage(String),
    GpuVram(String),
    NetRx(String),
    NetTx(String),
    DriveRead(String),
    DriveWrite(String),
    DriveBusy(String),
    VolRead(String),
    VolWrite(String),
    Sensor(String),
}

#[derive(Default)]
pub struct Model {
    pub st: Option<Static>,
    /// Per logical CPU, percent.
    pub usage: Vec<f32>,
    pub freq: Vec<u64>,
    pub total: f32,
    pub load: Option<(f64, f64, f64)>,
    pub cpu_hist: Ring,
    /// One ring per logical CPU: the heatmap's rows.
    pub threads: Vec<Ring>,
    pub mem: Mem,
    /// Used RAM and swap in percent of their totals.
    pub mem_hist: Ring,
    pub swap_hist: Ring,
    pub procs: Vec<Proc>,
    pub total_threads: Option<u64>,
    /// CPU and resident bytes per live process, for the detail column.
    pub proc_hist: HashMap<u32, (Ring, Ring)>,
    /// CPU readings so far.
    pub samples: u64,
    /// GPUs, then NPUs.
    pub gpus: Vec<gpu::Device>,
    /// Usage and VRAM in percent per live device, by its id.
    pub gpu_hist: HashMap<String, (Ring, Ring)>,
    /// Whether the GPUs have been read once (an empty list then means none).
    pub gpu_read: bool,
    /// The last reading of services, kept when the svc screen is left.
    pub services: svc::Services,
    /// Whether services have been read once.
    pub svc_read: bool,
    /// Network interfaces, up first.
    pub nics: Vec<net::Nic>,
    /// Received and sent bytes per second per live interface, by name.
    pub net_hist: HashMap<String, (Ring, Ring)>,
    pub nic_read: bool,
    /// The last reading of sockets, kept when the net screen is left.
    pub conns: net::Conns,
    pub conn_read: bool,
    pub disks: disk::Disks,
    /// Read and written bytes per second and busy percent per live drive,
    /// by its id.
    pub drive_hist: HashMap<String, (Ring, Ring, Ring)>,
    /// Read and written bytes per second per mounted volume, by mount point.
    pub vol_hist: HashMap<String, (Ring, Ring)>,
    pub disk_read: bool,
    pub sensors: sensors::Sensors,
    /// Each live sensor's values, by its id.
    pub sensor_hist: HashMap<String, Ring>,
    pub sensor_read: bool,
    /// Read once; `None` until its thread is done.
    pub hw: Option<hw::Hardware>,
    /// A laptop's; `None` without a battery.
    pub battery: Option<battery::Battery>,
    /// The last reading of devices, kept when the devices screen is left;
    /// `None` until read once.
    pub devices: Option<devices::Devices>,
    /// The last reading of containers; `None` until read once.
    pub containers: Option<containers::Containers>,
    /// Each live container's CPU percent and memory bytes, by id.
    pub ct_hist: HashMap<String, (Ring, Ring)>,
}

fn pct(part: u64, whole: u64) -> f32 {
    if whole == 0 { 0.0 } else { (part as f64 * 100.0 / whole as f64) as f32 }
}

impl Model {
    pub fn apply(&mut self, msg: Msg) {
        match msg {
            Msg::Static(st) => self.st = Some(*st),
            Msg::Cpu { usage, freq_mhz, total, load } => {
                if self.threads.len() != usage.len() {
                    self.threads = vec![Ring::default(); usage.len()];
                }
                for (r, &u) in self.threads.iter_mut().zip(&usage) {
                    r.push(u);
                }
                self.cpu_hist.grow().push(total);
                self.usage = usage;
                self.freq = freq_mhz;
                self.total = total;
                self.load = load;
                self.samples += 1;
            }
            Msg::Mem(m) => {
                self.mem_hist.grow().push(pct(m.used, m.total));
                self.swap_hist.grow().push(pct(m.swap_used, m.swap_total));
                self.mem = m;
            }
            Msg::Procs { procs, total_threads } => {
                let mut hist = std::mem::take(&mut self.proc_hist);
                hist.retain(|pid, _| procs.iter().any(|p| p.pid == *pid));
                for p in &procs {
                    let (c, m) = hist.entry(p.pid).or_default();
                    c.push(p.cpu);
                    m.push(p.mem as f32);
                }
                self.proc_hist = hist;
                self.procs = procs;
                self.total_threads = total_threads;
            }
            Msg::Gpu(devs) => {
                let mut hist = std::mem::take(&mut self.gpu_hist);
                hist.retain(|id, _| devs.iter().any(|d| d.id == *id));
                for d in &devs {
                    let (u, v) = hist.entry(d.id.clone()).or_insert_with(|| (Ring::long(), Ring::long()));
                    u.push(d.usage.unwrap_or(0.0));
                    v.push(d.vram_pct().unwrap_or(0.0));
                }
                self.gpu_hist = hist;
                self.gpus = devs;
                self.gpu_read = true;
            }
            Msg::Svc(s) => {
                self.services = s;
                self.svc_read = true;
            }
            Msg::Nics(nics) => {
                let mut hist = std::mem::take(&mut self.net_hist);
                hist.retain(|name, _| nics.iter().any(|n| n.name == *name));
                for n in &nics {
                    let (rx, tx) = hist.entry(n.name.clone()).or_insert_with(|| (Ring::long(), Ring::long()));
                    rx.push(n.rx_rate as f32);
                    tx.push(n.tx_rate as f32);
                }
                self.net_hist = hist;
                self.nics = nics;
                self.nic_read = true;
            }
            Msg::Conns(c) => {
                self.conns = c;
                self.conn_read = true;
            }
            Msg::Disks(d) => {
                let mut hist = std::mem::take(&mut self.drive_hist);
                hist.retain(|id, _| d.drives.iter().any(|x| x.id == *id));
                for x in &d.drives {
                    let (r, w, b) = hist.entry(x.id.clone()).or_insert_with(|| (Ring::long(), Ring::long(), Ring::long()));
                    r.push(x.read_rate as f32);
                    w.push(x.write_rate as f32);
                    b.push(x.busy.unwrap_or(0.0));
                }
                self.drive_hist = hist;
                let mut hist = std::mem::take(&mut self.vol_hist);
                hist.retain(|m, _| d.volumes.iter().any(|v| v.mount == *m));
                for v in &d.volumes {
                    let (r, w) = hist.entry(v.mount.clone()).or_insert_with(|| (Ring::long(), Ring::long()));
                    r.push(v.read_rate as f32);
                    w.push(v.write_rate as f32);
                }
                self.vol_hist = hist;
                self.disks = d;
                self.disk_read = true;
            }
            Msg::Sensors(s) => {
                let mut hist = std::mem::take(&mut self.sensor_hist);
                hist.retain(|id, _| s.list.iter().any(|x| x.id == *id));
                for x in &s.list {
                    hist.entry(x.id.clone()).or_insert_with(Ring::long).push(x.value);
                }
                self.sensor_hist = hist;
                self.sensors = s;
                self.sensor_read = true;
            }
            Msg::Hardware(h) => self.hw = Some(*h),
            Msg::Battery(b) => self.battery = b,
            Msg::Devices(d) => self.devices = Some(*d),
            Msg::Containers(c) => {
                let mut hist = std::mem::take(&mut self.ct_hist);
                hist.retain(|id, _| c.list.iter().any(|x| x.id == *id && x.state.live()));
                for x in c.list.iter().filter(|x| x.state.live()) {
                    let (cpu, mem) = hist.entry(x.id.clone()).or_default();
                    cpu.push(x.cpu.unwrap_or(0.0));
                    mem.push(x.mem.unwrap_or(0) as f32);
                }
                self.ct_hist = hist;
                self.containers = Some(*c);
            }
        }
    }

    pub fn series(&self, s: &Series) -> Option<&Ring> {
        Some(match s {
            Series::Cpu => &self.cpu_hist,
            Series::Mem => &self.mem_hist,
            Series::Swap => &self.swap_hist,
            Series::GpuUsage(id) => &self.gpu_hist.get(id)?.0,
            Series::GpuVram(id) => &self.gpu_hist.get(id)?.1,
            Series::NetRx(n) => &self.net_hist.get(n)?.0,
            Series::NetTx(n) => &self.net_hist.get(n)?.1,
            Series::DriveRead(id) => &self.drive_hist.get(id)?.0,
            Series::DriveWrite(id) => &self.drive_hist.get(id)?.1,
            Series::DriveBusy(id) => &self.drive_hist.get(id)?.2,
            Series::VolRead(m) => &self.vol_hist.get(m)?.0,
            Series::VolWrite(m) => &self.vol_hist.get(m)?.1,
            Series::Sensor(id) => self.sensor_hist.get(id)?,
        })
    }

    pub fn mem_pct(&self) -> f32 {
        pct(self.mem.used, self.mem.total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, cpu: f32) -> Proc {
        Proc {
            pid,
            ppid: None,
            name: format!("p{pid}"),
            cpu,
            mem: 1024,
            threads: Some(1),
            user: None,
            cmd: String::new(),
            exe: None,
            start: 0,
            status: "Run".into(),
        }
    }

    #[test]
    fn the_ring_keeps_the_newest_minute() {
        let mut r = Ring::default();
        for i in 0..100 {
            r.push(i as f32);
        }
        assert_eq!(r.len(), HISTORY);
        assert_eq!(r.iter().next(), Some(40.0));
        assert_eq!(r.last(), Some(99.0));
        assert_eq!(r.max(), 99.0);
        assert_eq!(r.min(), 40.0);
        assert_eq!(Ring::default().min(), 0.0);
    }

    #[test]
    fn the_long_ring_keeps_coarser_tiers() {
        let mut r = Ring::long();
        for i in 0..400 {
            r.push((i % 10) as f32);
        }
        // The short view is still the last minute.
        assert_eq!(r.len(), HISTORY);
        let five = r.span(Range::Min5);
        assert_eq!((five.len(), five[0], five[299]), (300, 0.0, 9.0));
        // Means of five and of thirty samples of 0..=9 repeating.
        let fifteen = r.span(Range::Min15);
        assert_eq!(fifteen.len(), 80);
        assert_eq!((fifteen[0], fifteen[1]), (2.0, 7.0));
        let hour = r.span(Range::Hour1);
        assert_eq!(hour.len(), 13);
        assert!(hour.iter().all(|&v| (v - 4.5).abs() < 1e-4));
        // A ring without tiers answers every range with its minute.
        let mut s = Ring::default();
        s.push(1.0);
        assert_eq!(s.span(Range::Hour1), vec![1.0]);
        for i in 0..5000 {
            r.push(i as f32);
        }
        assert_eq!((r.span(Range::Min5).len(), r.span(Range::Min15).len(), r.span(Range::Hour1).len()), (300, 180, 120));
    }

    #[test]
    fn ranges_step_and_stop_at_the_ends() {
        assert_eq!(Range::Min5.step(-1), Range::Min5);
        assert_eq!(Range::Min5.step(1), Range::Min15);
        assert_eq!(Range::Min15.step(1).step(1), Range::Hour1);
    }

    #[test]
    fn readings_fold_in_and_dead_processes_drop() {
        let mut m = Model::default();
        m.apply(Msg::Cpu { usage: vec![10.0, 30.0], freq_mhz: vec![3000, 3000], total: 20.0, load: None });
        assert_eq!(m.threads.len(), 2);
        assert_eq!(m.cpu_hist.last(), Some(20.0));
        m.apply(Msg::Mem(Mem { total: 400, used: 100, ..Mem::default() }));
        assert_eq!(m.mem_pct(), 25.0);
        m.apply(Msg::Procs { procs: vec![proc(1, 5.0), proc(2, 7.0)], total_threads: None });
        m.apply(Msg::Procs { procs: vec![proc(2, 9.0)], total_threads: None });
        assert!(!m.proc_hist.contains_key(&1));
        assert_eq!(m.proc_hist[&2].0.len(), 2);
    }

    #[test]
    fn gpu_history_follows_the_live_devices() {
        let dev = |id: &str, usage: f32| gpu::Device {
            id: id.into(),
            usage: Some(usage),
            vram_used: Some(1),
            vram_total: Some(4),
            ..gpu::Device::default()
        };
        let mut m = Model::default();
        assert!(!m.gpu_read);
        m.apply(Msg::Gpu(vec![dev("a", 10.0), dev("b", 20.0)]));
        m.apply(Msg::Gpu(vec![dev("b", 30.0)]));
        assert!(m.gpu_read);
        assert!(!m.gpu_hist.contains_key("a"));
        let (u, v) = &m.gpu_hist["b"];
        assert_eq!((u.len(), u.last(), v.last()), (2, Some(30.0), Some(25.0)));
    }

    #[test]
    fn net_history_follows_the_live_interfaces() {
        let nic = |name: &str, rx: f64| net::Nic { name: name.into(), rx_rate: rx, tx_rate: rx / 2.0, ..net::Nic::default() };
        let mut m = Model::default();
        m.apply(Msg::Nics(vec![nic("eth0", 100.0), nic("tun0", 5.0)]));
        m.apply(Msg::Nics(vec![nic("eth0", 300.0)]));
        assert!(m.nic_read && !m.net_hist.contains_key("tun0"));
        let (rx, tx) = &m.net_hist["eth0"];
        assert_eq!((rx.len(), rx.last(), tx.last()), (2, Some(300.0), Some(150.0)));
        assert!(!m.conn_read);
        m.apply(Msg::Conns(net::Conns { source: "/proc/net", ..net::Conns::default() }));
        assert!(m.conn_read);
    }

    #[test]
    fn disk_history_follows_the_live_drives_and_volumes() {
        let drive =
            |id: &str, r: f64| disk::Drive { id: id.into(), read_rate: r, write_rate: r * 2.0, busy: Some(5.0), ..disk::Drive::default() };
        let vol = |m: &str, r: f64| disk::Volume { mount: m.into(), read_rate: r, ..disk::Volume::default() };
        let mut m = Model::default();
        m.apply(Msg::Disks(disk::Disks {
            drives: vec![drive("sda", 1.0), drive("sdb", 2.0)],
            volumes: vec![vol("/", 3.0), vol("/mnt", 4.0)],
            note: None,
        }));
        m.apply(Msg::Disks(disk::Disks { drives: vec![drive("sda", 10.0)], volumes: vec![vol("/", 30.0)], note: None }));
        assert!(m.disk_read && !m.drive_hist.contains_key("sdb") && !m.vol_hist.contains_key("/mnt"));
        let (r, w, b) = &m.drive_hist["sda"];
        assert_eq!((r.len(), r.last(), w.last(), b.last()), (2, Some(10.0), Some(20.0), Some(5.0)));
        assert_eq!(m.vol_hist["/"].0.last(), Some(30.0));
    }

    #[test]
    fn sensor_history_follows_the_live_sensors() {
        let temp = |id: &str, v: f32| sensors::Sensor { id: id.into(), value: v, ..sensors::Sensor::default() };
        let mut m = Model::default();
        m.apply(Msg::Sensors(sensors::Sensors { list: vec![temp("a", 40.0), temp("b", 50.0)], note: None }));
        m.apply(Msg::Sensors(sensors::Sensors { list: vec![temp("a", 45.0)], note: None }));
        assert!(m.sensor_read && !m.sensor_hist.contains_key("b"));
        let a = &m.sensor_hist["a"];
        assert_eq!((a.len(), a.min(), a.max()), (2, 40.0, 45.0));
    }
}
