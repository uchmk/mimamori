//! `--snapshot` and `--log-metrics`: what the sampler reads, written out
//! without a window. The snapshot is every reading once as JSON; the metrics
//! log is a few numbers per row as CSV, to follow a long run.

use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::collect::{self, Ctl};
use crate::model::Model;

/// How long a snapshot waits for every source to have read once.
const PATIENCE: Duration = Duration::from_secs(15);

/// The sampling gap of a snapshot: rates need a second reading.
const QUICK: Duration = Duration::from_millis(500);

/// Readings after which the rates are there.
const WARM: u64 = 3;

#[derive(Serialize)]
struct Snapshot<'a> {
    /// Seconds since the epoch.
    time: u64,
    yagura: &'static str,
    system: &'a Option<collect::Static>,
    cpu: Cpu<'a>,
    mem: &'a collect::Mem,
    processes: &'a [collect::Proc],
    gpus: &'a [collect::gpu::Device],
    services: &'a collect::svc::Services,
    network: Network<'a>,
    disks: &'a collect::disk::Disks,
    sensors: &'a collect::sensors::Sensors,
    battery: &'a Option<collect::battery::Battery>,
    hardware: &'a Option<collect::hw::Hardware>,
    devices: &'a Option<collect::devices::Devices>,
    containers: &'a Option<collect::containers::Containers>,
}

#[derive(Serialize)]
struct Cpu<'a> {
    total: f32,
    /// Percent per logical CPU.
    usage: &'a [f32],
    freq_mhz: &'a [u64],
    /// The 1, 5 and 15 minute load average where the OS keeps one.
    load: Option<(f64, f64, f64)>,
}

#[derive(Serialize)]
struct Network<'a> {
    interfaces: &'a [collect::net::Nic],
    sockets: &'a collect::net::Conns,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Whether every source has been read once, so nothing in a snapshot is
/// missing only because it was early.
fn complete(m: &Model) -> bool {
    m.st.is_some()
        && m.samples >= WARM
        && !m.procs.is_empty()
        && m.gpu_read
        && m.svc_read
        && m.nic_read
        && m.conn_read
        && m.disk_read
        && m.sensor_read
        && m.hw.is_some()
        && m.devices.is_some()
        && m.containers.is_some()
}

/// Everything, as JSON.
pub fn snapshot() -> Result<String, String> {
    let (rx, ctl) = collect::spawn(None, QUICK);
    let _ = ctl.send(Ctl::Procs(1));
    let _ = ctl.send(Ctl::Services(true));
    let _ = ctl.send(Ctl::Net(true));
    let _ = ctl.send(Ctl::Devices(true));
    let _ = ctl.send(Ctl::Containers(true));
    let mut m = Model::default();
    let end = Instant::now() + PATIENCE;
    while !complete(&m) {
        let Some(left) = end.checked_duration_since(Instant::now()) else { break };
        match rx.recv_timeout(left) {
            Ok(msg) => m.apply(msg),
            Err(_) => break,
        }
    }
    let snap = Snapshot {
        time: now(),
        yagura: env!("CARGO_PKG_VERSION"),
        system: &m.st,
        cpu: Cpu { total: m.total, usage: &m.usage, freq_mhz: &m.freq, load: m.load },
        mem: &m.mem,
        processes: &m.procs,
        gpus: &m.gpus,
        services: &m.services,
        network: Network { interfaces: &m.nics, sockets: &m.conns },
        disks: &m.disks,
        sensors: &m.sensors,
        battery: &m.battery,
        hardware: &m.hw,
        devices: &m.devices,
        containers: &m.containers,
    };
    serde_json::to_string_pretty(&snap).map_err(|e| e.to_string())
}

/// The columns of the metrics log, in order.
const HEADER: &str = "time,cpu_pct,load1,mem_used_bytes,mem_total_bytes,swap_used_bytes,net_rx_bytes_s,net_tx_bytes_s,disk_read_bytes_s,\
disk_write_bytes_s,gpu_pct,temp_max_c,battery_pct,processes";

/// `2026-10-11T09:30:00Z` from seconds since the epoch (days to a calendar
/// date: Howard Hinnant's `civil_from_days`).
fn iso(t: u64) -> String {
    let (days, rem) = ((t / 86400) as i64, t % 86400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    format!("{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// A number, or nothing when the machine does not say.
fn opt(v: Option<f32>) -> String {
    v.map_or_else(String::new, |v| format!("{v:.1}"))
}

/// One row of the metrics log.
fn row(m: &Model, t: u64) -> String {
    let (rx, tx) = m.nics.iter().filter(|n| n.up).fold((0.0, 0.0), |(r, t), n| (r + n.rx_rate, t + n.tx_rate));
    let (read, write) = m.disks.rates();
    let gpu = m.gpus.iter().filter_map(|g| g.usage).max_by(f32::total_cmp);
    format!(
        "{},{:.1},{},{},{},{},{rx:.0},{tx:.0},{read:.0},{write:.0},{},{},{},{}",
        iso(t),
        m.total,
        m.load.map_or_else(String::new, |l| format!("{:.2}", l.0)),
        m.mem.used,
        m.mem.total,
        m.mem.swap_used,
        opt(gpu),
        opt(m.sensors.hottest().map(|s| s.value)),
        opt(m.battery.map(|b| b.pct)),
        m.procs.len(),
    )
}

/// Append a row to `path` every `every` until stopped (Ctrl+C); the header is
/// written when the file is new or empty.
pub fn log_metrics(path: &Path, every: Duration) -> Result<(), String> {
    let e = |e: std::io::Error| format!("{}: {e}", path.display());
    let fresh = std::fs::metadata(path).map_or(true, |m| m.len() == 0);
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(e)?;
    if fresh {
        writeln!(file, "{HEADER}").map_err(e)?;
    }
    let (rx, ctl) = collect::spawn(None, every);
    let _ = ctl.send(Ctl::Procs(1));
    let mut m = Model::default();
    let mut next = Instant::now() + every;
    loop {
        let wait = next.saturating_duration_since(Instant::now());
        match rx.recv_timeout(wait) {
            Ok(msg) => m.apply(msg),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return Err("the sampler stopped".into()),
        }
        if Instant::now() >= next {
            next += every;
            // The first readings have no rates yet.
            if m.samples >= 2 {
                writeln!(file, "{}", row(&m, now())).map_err(e)?;
                file.flush().map_err(e)?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_come_from_the_epoch() {
        assert_eq!(iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso(951_782_400 + 86_399), "2000-02-29T23:59:59Z");
        assert_eq!(iso(1_791_711_000), "2026-10-11T09:30:00Z");
    }

    #[test]
    fn a_row_has_a_cell_for_each_column() {
        let cols = HEADER.split(',').count();
        let line = row(&Model::default(), 0);
        assert_eq!(line.split(',').count(), cols, "{line}");
        // Nothing the machine does not say is made up.
        assert!(line.contains(",,"));
    }

    #[test]
    fn the_unnamed_make_no_cells() {
        assert_eq!(opt(None), "");
        assert_eq!(opt(Some(41.26)), "41.3");
    }

    #[test]
    fn a_snapshot_reads_as_json() {
        let m = Model::default();
        let snap = Snapshot {
            time: 1,
            yagura: "x",
            system: &m.st,
            cpu: Cpu { total: 0.0, usage: &m.usage, freq_mhz: &m.freq, load: None },
            mem: &m.mem,
            processes: &m.procs,
            gpus: &m.gpus,
            services: &m.services,
            network: Network { interfaces: &m.nics, sockets: &m.conns },
            disks: &m.disks,
            sensors: &m.sensors,
            battery: &m.battery,
            hardware: &m.hw,
            devices: &m.devices,
            containers: &m.containers,
        };
        let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&snap).unwrap()).unwrap();
        assert!(v["cpu"]["usage"].is_array() && v["mem"]["total"].is_number());
    }
}
