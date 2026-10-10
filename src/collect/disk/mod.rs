//! Disks: the physical drives with their traffic and how busy they are, and
//! the mounted volumes with their space and traffic. Read every tick, so the
//! graphs have a minute behind them when the disk screen opens.
//!
//! The volumes come from sysinfo on every OS. The drives come from
//! `/proc/diskstats` and `/sys/block` on Linux, and from the `PhysicalDisk`
//! performance counters (PDH) on Windows with each drive's model, bus and
//! kind asked of it once, and from IOKit's block storage drivers on macOS.
//! The parsing is kept in pure functions so the tests run everywhere.

#[cfg(windows)]
mod ioctl;
#[cfg(target_os = "macos")]
mod iokit;
#[cfg(any(target_os = "linux", test))]
pub mod linux;
#[cfg(any(target_os = "macos", test))]
pub mod mac;
#[cfg(any(windows, test))]
pub mod windows;

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use sysinfo::DiskRefreshKind;

/// Solid state or spinning, where the OS says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    Ssd,
    Hdd,
    #[default]
    Unknown,
}

impl Kind {
    #[cfg(test)]
    pub const ALL: [Kind; 3] = [Self::Ssd, Self::Hdd, Self::Unknown];

    pub fn key(self) -> &'static str {
        match self {
            Self::Ssd => "disk.kind.ssd",
            Self::Hdd => "disk.kind.hdd",
            Self::Unknown => "disk.kind.unknown",
        }
    }

    fn from_sysinfo(k: sysinfo::DiskKind) -> Self {
        match k {
            sysinfo::DiskKind::SSD => Self::Ssd,
            sysinfo::DiskKind::HDD => Self::Hdd,
            sysinfo::DiskKind::Unknown(_) => Self::Unknown,
        }
    }
}

/// Why the drive list is empty or short.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// Another OS: the drives are not read; each volume shows its own
    /// traffic.
    #[cfg_attr(any(windows, target_os = "linux", target_os = "macos"), allow(dead_code))]
    NotRead,
    /// Windows: the `PhysicalDisk` counters are not there (disabled).
    #[cfg_attr(not(windows), allow(dead_code))]
    CountersOff,
    /// Linux: `/proc/diskstats` could not be read.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    NoDiskstats,
    /// macOS: IOKit could not list the block storage drivers.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    NoIoKit,
}

impl Note {
    #[cfg(test)]
    pub const ALL: [Note; 4] = [Self::NotRead, Self::CountersOff, Self::NoDiskstats, Self::NoIoKit];

    pub fn key(self) -> &'static str {
        match self {
            Self::NotRead => "disk.note.not_read",
            Self::CountersOff => "disk.note.counters_off",
            Self::NoDiskstats => "disk.note.no_diskstats",
            Self::NoIoKit => "disk.note.no_iokit",
        }
    }
}

/// A physical drive (or what the OS shows as one: a virtual disk, an LVM or
/// RAID device).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drive {
    /// `sda`, `nvme0n1`; Windows' disk number.
    pub id: String,
    /// `sda`; `Disk 0` on Windows.
    pub name: String,
    /// The model the drive reports; empty when there is none.
    pub model: String,
    pub kind: Kind,
    pub bus: Option<&'static str>,
    pub size: Option<u64>,
    pub removable: bool,
    /// Bytes since boot, where the OS keeps a count (not Windows).
    pub read_total: Option<u64>,
    pub write_total: Option<u64>,
    /// Bytes per second since the last reading; 0 on the first.
    pub read_rate: f64,
    pub write_rate: f64,
    /// Percent of the time the drive had work.
    pub busy: Option<f32>,
    /// The mount points of the volumes on it.
    pub volumes: Vec<String>,
}

/// A mounted file system.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Volume {
    /// `/`, `C:\`.
    pub mount: String,
    /// The device (`/dev/sda2`) or the label (`Windows`).
    pub name: String,
    pub fs: String,
    pub total: u64,
    pub avail: u64,
    pub kind: Kind,
    pub removable: bool,
    pub read_only: bool,
    /// Bytes per second since the last reading; 0 on the first.
    pub read_rate: f64,
    pub write_rate: f64,
    /// The [`Drive::id`] it is on, when known.
    pub drive: Option<String>,
}

impl Volume {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.avail)
    }

    /// Percent used; `None` for an empty size.
    pub fn use_pct(&self) -> Option<f32> {
        (self.total > 0).then(|| self.used() as f32 * 100.0 / self.total as f32)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Disks {
    pub drives: Vec<Drive>,
    pub volumes: Vec<Volume>,
    pub note: Option<Note>,
}

impl Disks {
    /// Bytes per second read and written, over the drives (or the volumes,
    /// where there are no drives).
    pub fn rates(&self) -> (f64, f64) {
        if self.drives.is_empty() {
            self.volumes.iter().fold((0.0, 0.0), |(r, w), v| (r + v.read_rate, w + v.write_rate))
        } else {
            self.drives.iter().fold((0.0, 0.0), |(r, w), d| (r + d.read_rate, w + d.write_rate))
        }
    }
}

/// Fill in each drive's volumes from the volumes' drives.
fn link(drives: &mut [Drive], volumes: &[Volume]) {
    for d in drives {
        d.volumes = volumes.iter().filter(|v| v.drive.as_deref() == Some(d.id.as_str())).map(|v| v.mount.clone()).collect();
    }
}

/// Reads the drives and the volumes and works out their rates.
pub struct Reader {
    vols: sysinfo::Disks,
    /// The mount points read last time: a new one has no rate yet (its first
    /// count is everything since boot).
    seen: HashSet<PathBuf>,
    vol_at: Option<Instant>,
    #[cfg(target_os = "linux")]
    linux: linux::Reader,
    #[cfg(windows)]
    win: Option<ioctl::Reader>,
    #[cfg(target_os = "macos")]
    mac: iokit::Reader,
}

impl Reader {
    pub fn new() -> Self {
        Self {
            vols: sysinfo::Disks::new_with_refreshed_list_specifics(DiskRefreshKind::everything()),
            seen: HashSet::new(),
            vol_at: None,
            #[cfg(target_os = "linux")]
            linux: linux::Reader::default(),
            #[cfg(windows)]
            win: ioctl::Reader::open(),
            #[cfg(target_os = "macos")]
            mac: iokit::Reader::default(),
        }
    }

    pub fn read(&mut self) -> Disks {
        let (mut drives, note) = self.drives();
        let mut volumes = self.volumes();
        for v in &mut volumes {
            v.drive = self.drive_of(v, &drives);
        }
        link(&mut drives, &volumes);
        Disks { drives, volumes, note }
    }

    fn volumes(&mut self) -> Vec<Volume> {
        // The kind is read once per new disk by sysinfo itself.
        self.vols.refresh_specifics(true, DiskRefreshKind::nothing().with_kind().with_storage().with_io_usage());
        let now = Instant::now();
        let secs = self.vol_at.map(|t| now.duration_since(t).as_secs_f64()).filter(|&s| s > 0.0);
        let mut out: Vec<Volume> = self
            .vols
            .list()
            .iter()
            .map(|d| {
                let u = d.usage();
                let known = self.seen.contains(d.mount_point());
                let rate = |b: u64| match secs {
                    Some(s) if known => b as f64 / s,
                    _ => 0.0,
                };
                Volume {
                    mount: d.mount_point().display().to_string(),
                    name: d.name().to_string_lossy().into_owned(),
                    fs: d.file_system().to_string_lossy().into_owned(),
                    total: d.total_space(),
                    avail: d.available_space(),
                    kind: Kind::from_sysinfo(d.kind()),
                    removable: d.is_removable(),
                    read_only: d.is_read_only(),
                    read_rate: rate(u.read_bytes),
                    write_rate: rate(u.written_bytes),
                    drive: None,
                }
            })
            .collect();
        self.seen = self.vols.list().iter().map(|d| d.mount_point().to_path_buf()).collect();
        self.vol_at = Some(now);
        out.sort_by(|a, b| a.mount.cmp(&b.mount));
        out
    }

    #[cfg(target_os = "linux")]
    fn drives(&mut self) -> (Vec<Drive>, Option<Note>) {
        self.linux.read(std::path::Path::new("/sys/block"))
    }

    #[cfg(windows)]
    fn drives(&mut self) -> (Vec<Drive>, Option<Note>) {
        match &mut self.win {
            Some(r) => (r.read(), None),
            None => (Vec::new(), Some(Note::CountersOff)),
        }
    }

    #[cfg(target_os = "macos")]
    fn drives(&mut self) -> (Vec<Drive>, Option<Note>) {
        self.mac.read()
    }

    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    fn drives(&mut self) -> (Vec<Drive>, Option<Note>) {
        (Vec::new(), Some(Note::NotRead))
    }

    /// Which drive a volume is on: by the device it was mounted from on
    /// Linux, by its drive letter on Windows, by walking IOKit up from its
    /// device on macOS.
    #[cfg(target_os = "linux")]
    fn drive_of(&mut self, v: &Volume, drives: &[Drive]) -> Option<String> {
        // `/dev/disk/by-uuid/…` and `/dev/mapper/…` are links to the real node.
        let dev = std::fs::canonicalize(&v.name).ok()?;
        let name = dev.strip_prefix("/dev").ok()?.to_str()?;
        let ids: Vec<&str> = drives.iter().map(|d| d.id.as_str()).collect();
        linux::owner(name, &ids).map(str::to_string)
    }

    #[cfg(windows)]
    fn drive_of(&mut self, v: &Volume, drives: &[Drive]) -> Option<String> {
        let letter = windows::letter(&v.mount)?;
        drives.iter().find(|d| d.volumes.iter().any(|l| l.eq_ignore_ascii_case(&letter))).map(|d| d.id.clone())
    }

    #[cfg(target_os = "macos")]
    fn drive_of(&mut self, v: &Volume, drives: &[Drive]) -> Option<String> {
        // The drive must still be listed: one unplugged since keeps no volumes.
        self.mac.drive_of(&v.mount).filter(|id| drives.iter().any(|d| &d.id == id))
    }

    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    fn drive_of(&mut self, _: &Volume, _: &[Drive]) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::tr;

    #[test]
    fn every_kind_and_note_has_english() {
        for k in Kind::ALL {
            assert_ne!(tr(k.key()), k.key());
        }
        for n in Note::ALL {
            assert_ne!(tr(n.key()), n.key());
        }
    }

    #[test]
    fn a_drive_lists_the_volumes_on_it() {
        let mut drives = vec![Drive { id: "sda".into(), ..Drive::default() }, Drive { id: "sdb".into(), ..Drive::default() }];
        let vol = |m: &str, d: Option<&str>| Volume { mount: m.into(), drive: d.map(Into::into), ..Volume::default() };
        let volumes = [vol("/", Some("sda")), vol("/home", Some("sda")), vol("/mnt/usb", Some("sdb")), vol("/net", None)];
        link(&mut drives, &volumes);
        assert_eq!(drives[0].volumes, ["/", "/home"]);
        assert_eq!(drives[1].volumes, ["/mnt/usb"]);
    }

    #[test]
    fn rates_add_up_the_drives_or_else_the_volumes() {
        let mut d = Disks { volumes: vec![Volume { read_rate: 1.0, write_rate: 2.0, ..Volume::default() }; 2], ..Disks::default() };
        assert_eq!(d.rates(), (2.0, 4.0));
        d.drives = vec![Drive { read_rate: 10.0, write_rate: 20.0, ..Drive::default() }];
        assert_eq!(d.rates(), (10.0, 20.0));
    }

    #[test]
    fn a_volume_knows_how_full_it_is() {
        let v = Volume { total: 200, avail: 50, ..Volume::default() };
        assert_eq!(v.used(), 150);
        assert_eq!(v.use_pct(), Some(75.0));
        assert_eq!(Volume::default().use_pct(), None);
    }
}
