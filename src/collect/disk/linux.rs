//! Linux: the drives from `/proc/diskstats` (traffic and busy time) and
//! `/sys/block` (which entries are whole drives, their size, model, bus and
//! kind).

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use super::{Drive, Kind, Note};

/// One line of `/proc/diskstats`, in bytes and milliseconds.
#[derive(Clone, Debug, PartialEq)]
pub struct Stat {
    pub name: String,
    pub read: u64,
    pub write: u64,
    /// Milliseconds spent with I/O in flight (`io_ticks`).
    pub ticks: u64,
}

/// Sectors in diskstats and sysfs are 512 bytes whatever the drive's own.
const SECTOR: u64 = 512;

pub fn parse(text: &str) -> Vec<Stat> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            // major minor name, then reads merged sectors ms, writes merged
            // sectors ms, in-flight, io_ticks, …
            let num = |i: usize| f.get(i)?.parse::<u64>().ok();
            Some(Stat { name: f.get(2)?.to_string(), read: num(5)? * SECTOR, write: num(9)? * SECTOR, ticks: num(12)? })
        })
        .collect()
}

/// Block devices that are not drives: loop files, RAM disks, compressed swap.
pub fn skip(name: &str) -> bool {
    ["loop", "ram", "zram"].iter().any(|p| name.starts_with(p))
}

/// The bus from the device name and where it sits in sysfs.
pub fn bus(name: &str, path: &str) -> Option<&'static str> {
    let starts = |p: &str| name.starts_with(p);
    Some(if path.contains("/usb") {
        "USB"
    } else if starts("nvme") {
        "NVMe"
    } else if starts("mmcblk") {
        "SD / MMC"
    } else if starts("vd") {
        "virtio"
    } else if starts("xvd") {
        "Xen"
    } else if starts("dm-") {
        "device-mapper"
    } else if starts("md") {
        "md RAID"
    } else if starts("sr") {
        "optical"
    } else if starts("sd") {
        "SATA / SCSI"
    } else {
        return None;
    })
}

/// Spinning or not from `queue/rotational`. A virtual disk's answer is the
/// host's guess, so it reads as unknown.
pub fn kind(rotational: Option<&str>, bus: Option<&str>) -> Kind {
    if matches!(bus, Some("virtio" | "Xen" | "device-mapper" | "md RAID")) {
        return Kind::Unknown;
    }
    match rotational.map(str::trim) {
        Some("1") => Kind::Hdd,
        Some("0") => Kind::Ssd,
        _ => Kind::Unknown,
    }
}

/// The drive a device node is, or is a partition of: `sda2` on `sda`,
/// `nvme0n1p1` on `nvme0n1`, `mmcblk0p2` on `mmcblk0`.
pub fn owner<'a>(dev: &str, ids: &[&'a str]) -> Option<&'a str> {
    ids.iter()
        .filter(|id| match dev.strip_prefix(**id) {
            Some("") => true,
            Some(rest) => {
                let rest = rest.strip_prefix('p').filter(|_| id.ends_with(|c: char| c.is_ascii_digit())).unwrap_or(rest);
                !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
            }
            None => false,
        })
        .max_by_key(|id| id.len())
        .copied()
}

/// The whole drives among the stats, filled in from sysfs. Each comes with
/// its busy milliseconds, for the next reading's rate.
pub fn drives(sys_block: &Path, stats: &[Stat]) -> Vec<(Drive, u64)> {
    let read = |p: &Path| std::fs::read_to_string(p).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let mut out: Vec<(Drive, u64)> = stats
        .iter()
        .filter(|s| !skip(&s.name))
        .filter_map(|s| {
            let dir = sys_block.join(&s.name);
            // Partitions are in diskstats but not in /sys/block.
            if !dir.is_dir() {
                return None;
            }
            let size = read(&dir.join("size")).and_then(|v| v.parse::<u64>().ok()).map(|n| n * SECTOR);
            // An empty card reader or optical drive.
            if size == Some(0) {
                return None;
            }
            let path = std::fs::canonicalize(&dir).map(|p| p.display().to_string()).unwrap_or_default();
            let bus = bus(&s.name, &path);
            let model = read(&dir.join("device/model"))
                .or_else(|| read(&dir.join("dm/name")))
                .or_else(|| read(&dir.join("md/level")))
                .unwrap_or_default();
            let drive = Drive {
                id: s.name.clone(),
                name: s.name.clone(),
                model,
                kind: kind(read(&dir.join("queue/rotational")).as_deref(), bus),
                bus,
                size,
                removable: read(&dir.join("removable")).as_deref() == Some("1"),
                read_total: Some(s.read),
                write_total: Some(s.write),
                ..Drive::default()
            };
            Some((drive, s.ticks))
        })
        .collect();
    out.sort_by(|a, b| a.0.name.cmp(&b.0.name));
    out
}

/// Fill in the rates and busy share from the counts of the last reading,
/// `secs` ago.
pub fn rates(list: &mut [(Drive, u64)], last: &HashMap<String, (u64, u64, u64)>, secs: Option<f64>) {
    let Some(secs) = secs.filter(|&s| s > 0.0) else { return };
    for (d, ticks) in list {
        let Some(&(r, w, t)) = last.get(&d.id) else { continue };
        d.read_rate = d.read_total.unwrap_or(0).saturating_sub(r) as f64 / secs;
        d.write_rate = d.write_total.unwrap_or(0).saturating_sub(w) as f64 / secs;
        d.busy = Some((ticks.saturating_sub(t) as f64 / (secs * 10.0)).min(100.0) as f32);
    }
}

#[derive(Default)]
pub struct Reader {
    last: HashMap<String, (u64, u64, u64)>,
    at: Option<Instant>,
}

impl Reader {
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn read(&mut self, sys_block: &Path) -> (Vec<Drive>, Option<Note>) {
        let Ok(text) = std::fs::read_to_string("/proc/diskstats") else { return (Vec::new(), Some(Note::NoDiskstats)) };
        let mut list = drives(sys_block, &parse(&text));
        let now = Instant::now();
        rates(&mut list, &self.last, self.at.map(|t| now.duration_since(t).as_secs_f64()));
        self.last = list.iter().map(|(d, t)| (d.id.clone(), (d.read_total.unwrap_or(0), d.write_total.unwrap_or(0), *t))).collect();
        self.at = Some(now);
        (list.into_iter().map(|(d, _)| d).collect(), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATS: &str = "\
   7       0 loop0 10 0 80 0 0 0 0 0 0 0 0 0 0 0 0 0 0
 254       0 vda 53601 24113 5743114 258378 10166 10314 21495664 84461 0 229144 355162 34564 0 40964768 11763 138 557
 254       1 vda1 100 0 2000 10 5 0 40 1 0 20 11
 259       0 nvme0n1 1 0 8 0 2 0 16 0 0 7 0
 short line
";

    #[test]
    fn diskstats_lines_become_bytes_and_busy_time() {
        let s = parse(STATS);
        assert_eq!(s.len(), 4);
        assert_eq!(s[1], Stat { name: "vda".into(), read: 5743114 * 512, write: 21495664 * 512, ticks: 229144 });
        assert_eq!(s[3], Stat { name: "nvme0n1".into(), read: 8 * 512, write: 16 * 512, ticks: 7 });
    }

    #[test]
    fn loops_ram_disks_and_zram_are_not_drives() {
        assert!(skip("loop3") && skip("ram0") && skip("zram0"));
        assert!(!skip("sda") && !skip("nvme0n1"));
    }

    #[test]
    fn the_bus_comes_from_the_name_or_a_usb_path() {
        assert_eq!(bus("sda", "/sys/devices/pci0000:00/0000:00:17.0/ata1/host0/target0:0:0/0:0:0:0/block/sda"), Some("SATA / SCSI"));
        assert_eq!(bus("sdb", "/sys/devices/pci0000:00/0000:00:14.0/usb2/2-1/2-1:1.0/host6/block/sdb"), Some("USB"));
        assert_eq!(bus("nvme0n1", ""), Some("NVMe"));
        assert_eq!(bus("mmcblk0", ""), Some("SD / MMC"));
        assert_eq!(bus("vda", ""), Some("virtio"));
        assert_eq!(bus("dm-0", ""), Some("device-mapper"));
        assert_eq!(bus("weird0", ""), None);
    }

    #[test]
    fn rotational_says_the_kind_except_on_virtual_disks() {
        assert_eq!(kind(Some("0\n"), Some("NVMe")), Kind::Ssd);
        assert_eq!(kind(Some("1"), Some("SATA / SCSI")), Kind::Hdd);
        assert_eq!(kind(Some("1"), Some("virtio")), Kind::Unknown);
        assert_eq!(kind(None, None), Kind::Unknown);
    }

    #[test]
    fn a_partition_belongs_to_its_drive() {
        let ids = ["sda", "sdab", "nvme0n1", "nvme0n10", "mmcblk0", "dm-0"];
        assert_eq!(owner("sda", &ids), Some("sda"));
        assert_eq!(owner("sda2", &ids), Some("sda"));
        assert_eq!(owner("sdab1", &ids), Some("sdab"));
        assert_eq!(owner("nvme0n1p3", &ids), Some("nvme0n1"));
        assert_eq!(owner("nvme0n10p1", &ids), Some("nvme0n10"));
        assert_eq!(owner("mmcblk0p1", &ids), Some("mmcblk0"));
        assert_eq!(owner("dm-0", &ids), Some("dm-0"));
        // `sdc` is not a partition of `sd…` anything listed, and `sdap` is no partition name.
        assert_eq!(owner("sdc1", &ids), None);
        assert_eq!(owner("sdap", &ids), None);
    }

    #[test]
    fn drives_are_the_whole_disks_in_sysfs() {
        let root = crate::util::test_dir("disk-sysfs");
        let vda = root.join("vda");
        std::fs::create_dir_all(vda.join("queue")).unwrap();
        std::fs::write(vda.join("size"), "536870912\n").unwrap();
        std::fs::write(vda.join("queue/rotational"), "1\n").unwrap();
        std::fs::write(vda.join("removable"), "0\n").unwrap();
        let nvme = root.join("nvme0n1");
        std::fs::create_dir_all(nvme.join("device")).unwrap();
        std::fs::create_dir_all(nvme.join("queue")).unwrap();
        std::fs::write(nvme.join("size"), "2000\n").unwrap();
        std::fs::write(nvme.join("device/model"), "Samsung SSD 990 PRO 2TB   \n").unwrap();
        std::fs::write(nvme.join("queue/rotational"), "0\n").unwrap();
        // loop0 is skipped by name and vda1 is a partition (not in the tree).
        std::fs::create_dir_all(root.join("loop0")).unwrap();
        let list = drives(&root, &parse(STATS));
        let names: Vec<&str> = list.iter().map(|(d, _)| d.name.as_str()).collect();
        assert_eq!(names, ["nvme0n1", "vda"]);
        let (nv, ticks) = &list[0];
        assert_eq!(nv.model, "Samsung SSD 990 PRO 2TB");
        assert_eq!((nv.kind, nv.bus, nv.size), (Kind::Ssd, Some("NVMe"), Some(2000 * 512)));
        assert_eq!(*ticks, 7);
        let (vd, _) = &list[1];
        assert_eq!((vd.kind, vd.bus, vd.removable), (Kind::Unknown, Some("virtio"), false));
        assert_eq!(vd.read_total, Some(5743114 * 512));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rates_and_busy_come_from_the_last_counts() {
        let d = |r: u64, w: u64| Drive { id: "sda".into(), read_total: Some(r), write_total: Some(w), ..Drive::default() };
        let mut list = vec![(d(3000, 5000), 1500), (Drive { id: "new".into(), ..Drive::default() }, 0)];
        let last = HashMap::from([("sda".to_string(), (1000, 6000, 1000))]);
        rates(&mut list, &last, Some(2.0));
        assert_eq!(list[0].0.read_rate, 1000.0);
        // A count that went back (the drive was reset) reads as 0.
        assert_eq!(list[0].0.write_rate, 0.0);
        assert_eq!(list[0].0.busy, Some(25.0));
        assert_eq!(list[1].0.busy, None);
        // Busy is capped: io_ticks can run ahead of the clock.
        let mut list = vec![(d(0, 0), 9000)];
        rates(&mut list, &HashMap::from([("sda".to_string(), (0, 0, 0))]), Some(1.0));
        assert_eq!(list[0].0.busy, Some(100.0));
    }
}
