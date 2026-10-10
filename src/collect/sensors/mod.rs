//! Sensors: temperatures and fans, grouped by the chip that has them. Read
//! every tick, so each sensor has a minute behind it when the screen opens.
//!
//! Linux reads `/sys/class/hwmon` (temperatures and fans with their limits),
//! falling back to `/sys/class/thermal` where no chip has a temperature.
//! Windows reads the ACPI thermal zones from the `Thermal Zone Information`
//! performance counters (PDH), which need no administrator; it has no common
//! way to read fans. macOS reads the SMC itself (the named temperatures and
//! the fans with their least and most) and the HID sensors on Apple silicon
//! through sysinfo's components, with friendlier names. NVIDIA GPUs add their NVML temperature and fan, and the drives
//! the temperature the disk reading found, where the OS does not list them
//! already. The parsing is kept in pure functions so the
//! tests run everywhere.

#[cfg(any(target_os = "linux", test))]
pub mod linux;
#[cfg(any(target_os = "macos", test))]
pub mod mac;
#[cfg(target_os = "macos")]
mod smc;
#[cfg(any(windows, test))]
pub mod windows;
#[cfg(windows)]
mod zones;

use crate::collect::{disk, gpu};

/// What a sensor measures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// Degrees Celsius.
    #[default]
    Temp,
    /// Revolutions per minute.
    Fan,
    /// A fan's speed as a percent of its most (NVML).
    Duty,
}

impl Kind {
    #[cfg(test)]
    pub const ALL: [Kind; 3] = [Self::Temp, Self::Fan, Self::Duty];

    pub fn key(self) -> &'static str {
        match self {
            Self::Temp => "sensors.kind.temp",
            Self::Fan => "sensors.kind.fan",
            Self::Duty => "sensors.kind.duty",
        }
    }

    /// A value with its unit.
    pub fn show(self, v: f32) -> String {
        match self {
            Self::Temp => format!("{v:.0}°C"),
            Self::Fan => format!("{v:.0} rpm"),
            Self::Duty => format!("{v:.0}%"),
        }
    }
}

/// Why the list is empty or short.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// Nothing to read: a virtual machine, a container, or no driver for
    /// the chip.
    Empty,
    /// Windows: only the ACPI thermal zones; fans and the cores need the
    /// maker's driver.
    #[cfg_attr(not(windows), allow(dead_code))]
    ZonesOnly,
    /// Linux: no hwmon chip has a temperature; the thermal zones instead.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    ThermalOnly,
}

impl Note {
    #[cfg(test)]
    pub const ALL: [Note; 3] = [Self::Empty, Self::ZonesOnly, Self::ThermalOnly];

    pub fn key(self) -> &'static str {
        match self {
            Self::Empty => "sensors.note.empty",
            Self::ZonesOnly => "sensors.note.zones_only",
            Self::ThermalOnly => "sensors.note.thermal_only",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sensor {
    /// Unique while running: `hwmon2/temp1`, the zone's instance name.
    pub id: String,
    /// The chip it belongs to; sensors of one chip are listed together.
    pub chip_id: String,
    /// `coretemp`, `nvme Samsung SSD 980`, `Thermal zone`.
    pub chip: String,
    /// `Package id 0`, `temp1`, `fan2`.
    pub label: String,
    pub kind: Kind,
    pub value: f32,
    /// Where it should stay below: the chip's high mark (temperatures) or
    /// its most (fans).
    pub high: Option<f32>,
    /// Where the chip shuts down or throttles hard.
    pub crit: Option<f32>,
    /// A fan's least before it counts as failed.
    pub min: Option<f32>,
    /// Where it was read: a sysfs file, a counter, `NVML`.
    pub source: String,
    /// The PCI address of the device behind the chip, for matching GPUs.
    pub pci: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sensors {
    pub list: Vec<Sensor>,
    pub note: Option<Note>,
}

/// A run of sensors of one chip in [`Sensors::list`].
#[derive(Clone, Debug, PartialEq)]
pub struct Chip<'a> {
    pub name: &'a str,
    pub first: usize,
    pub len: usize,
}

impl Sensors {
    /// The chips in list order.
    pub fn chips(&self) -> Vec<Chip<'_>> {
        let mut out: Vec<Chip> = Vec::new();
        let mut last: Option<&str> = None;
        for (i, s) in self.list.iter().enumerate() {
            match out.last_mut() {
                Some(c) if last == Some(s.chip_id.as_str()) => c.len += 1,
                _ => out.push(Chip { name: &s.chip, first: i, len: 1 }),
            }
            last = Some(&s.chip_id);
        }
        out
    }

    /// The hottest temperature.
    pub fn hottest(&self) -> Option<&Sensor> {
        self.list.iter().filter(|s| s.kind == Kind::Temp).max_by(|a, b| a.value.total_cmp(&b.value))
    }

    pub fn count(&self, k: Kind) -> usize {
        self.list.iter().filter(|s| s.kind == k).count()
    }
}

/// Add the GPUs' temperatures and fans that no chip already lists (the
/// NVIDIA driver registers no hwmon chip; amdgpu does, and is matched by its
/// PCI address).
fn add_gpus(list: &mut Vec<Sensor>, gpus: &[gpu::Device]) {
    for d in gpus {
        if d.temp.is_none() && d.fan.is_none() {
            continue;
        }
        let pci = d.bus.as_deref().map(str::to_ascii_lowercase);
        if pci.is_some() && list.iter().any(|s| s.pci == pci) {
            continue;
        }
        let base = Sensor {
            chip_id: format!("gpu:{}", d.id),
            chip: d.name.clone(),
            source: d.source.to_string(),
            pci: pci.clone(),
            ..Sensor::default()
        };
        if let Some(t) = d.temp {
            list.push(Sensor { id: format!("gpu:{}/temp", d.id), label: "GPU".into(), value: t, ..base.clone() });
        }
        match d.fan {
            Some(gpu::Fan::Rpm(r)) => {
                list.push(Sensor { id: format!("gpu:{}/fan", d.id), label: "fan".into(), kind: Kind::Fan, value: r as f32, ..base })
            }
            Some(gpu::Fan::Percent(p)) => {
                list.push(Sensor { id: format!("gpu:{}/fan", d.id), label: "fan".into(), kind: Kind::Duty, value: p as f32, ..base })
            }
            None => {}
        }
    }
}

/// Add the drives' temperatures that no chip already lists (a drive's hwmon
/// chip on Linux is matched by its name; Windows' drive temperatures and
/// `smartctl`'s come from the disk reading alone).
fn add_drives(list: &mut Vec<Sensor>, drives: &[disk::Drive]) {
    for d in drives {
        let Some(t) = &d.temp else { continue };
        if t.chip.as_ref().is_some_and(|c| list.iter().any(|s| &s.chip_id == c)) {
            continue;
        }
        list.push(Sensor {
            id: format!("disk:{}/temp", d.id),
            chip_id: format!("disk:{}", d.id),
            chip: if d.model.is_empty() { d.name.clone() } else { d.model.clone() },
            label: d.name.clone(),
            value: t.value,
            high: t.high,
            crit: t.crit,
            source: t.source.clone(),
            ..Sensor::default()
        });
    }
}

/// Reads the sensors, keeping what each OS opens once.
pub struct Reader {
    #[cfg(windows)]
    zones: Option<zones::Reader>,
    #[cfg(target_os = "macos")]
    comps: sysinfo::Components,
    #[cfg(target_os = "macos")]
    smc: Option<smc::Smc>,
}

impl Reader {
    pub fn new() -> Self {
        Self {
            #[cfg(windows)]
            zones: zones::Reader::open(),
            #[cfg(target_os = "macos")]
            comps: sysinfo::Components::new_with_refreshed_list(),
            #[cfg(target_os = "macos")]
            smc: smc::Smc::open(),
        }
    }

    /// `gpus` and `drives` are this tick's GPU and drive readings.
    pub fn read(&mut self, gpus: &[gpu::Device], drives: &[disk::Drive]) -> Sensors {
        let (mut list, note) = self.chips();
        add_gpus(&mut list, gpus);
        add_drives(&mut list, drives);
        let note = if list.is_empty() && note.is_none() { Some(Note::Empty) } else { note };
        Sensors { list, note }
    }

    #[cfg(target_os = "linux")]
    fn chips(&mut self) -> (Vec<Sensor>, Option<Note>) {
        linux::read(std::path::Path::new("/sys/class"))
    }

    #[cfg(windows)]
    fn chips(&mut self) -> (Vec<Sensor>, Option<Note>) {
        (self.zones.as_mut().map(zones::Reader::read).unwrap_or_default(), Some(Note::ZonesOnly))
    }

    #[cfg(target_os = "macos")]
    fn chips(&mut self) -> (Vec<Sensor>, Option<Note>) {
        self.comps.refresh(true);
        let mut list = self.smc.as_ref().map(smc::Smc::read_all).unwrap_or_default();
        let smc = self.smc.as_ref();
        list.extend(self.comps.list().iter().filter_map(|c| {
            let raw = c.label();
            let id = c.id().unwrap_or(raw);
            // Intel: sysinfo's components are SMC keys the reader lists.
            if smc.is_some_and(|s| s.has(id) || mac::is_smc_key(id)) {
                return None;
            }
            let (chip_id, chip, label, source) = if mac::is_smc_key(id) {
                ("smc", "SMC", raw.to_string(), format!("SMC {id}"))
            } else {
                ("hid", "HID sensors", mac::hid_name(raw)?, format!("IOHID {raw}"))
            };
            Some(Sensor {
                id: format!("{chip_id}/{id}"),
                chip_id: chip_id.into(),
                chip: chip.into(),
                value: c.temperature().filter(|t| t.is_finite())?,
                crit: c.critical(),
                source,
                label,
                ..Sensor::default()
            })
        }));
        (list, None)
    }

    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    fn chips(&mut self) -> (Vec<Sensor>, Option<Note>) {
        (Vec::new(), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::tr;

    fn temp(chip: &str, label: &str, v: f32) -> Sensor {
        Sensor {
            id: format!("{chip}/{label}"),
            chip_id: chip.into(),
            chip: chip.into(),
            label: label.into(),
            value: v,
            ..Sensor::default()
        }
    }

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
    fn chips_are_the_runs_of_one_chip() {
        let mut s = Sensors { list: vec![temp("a", "t1", 40.0), temp("a", "t2", 70.0), temp("b", "t1", 50.0)], note: None };
        // Two chips with one name (two of the same drive) stay apart.
        s.list.push(Sensor { chip_id: "c".into(), ..temp("b", "t1", 30.0) });
        let chips = s.chips();
        assert_eq!(chips.len(), 3);
        assert_eq!((chips[0].name, chips[0].first, chips[0].len), ("a", 0, 2));
        assert_eq!((chips[2].name, chips[2].first, chips[2].len), ("b", 3, 1));
        assert_eq!(s.hottest().map(|x| x.value), Some(70.0));
        assert_eq!(s.count(Kind::Temp), 4);
    }

    #[test]
    fn gpus_add_what_no_chip_lists() {
        let gpu = |id: &str, bus: Option<&str>, fan| gpu::Device {
            id: id.into(),
            name: format!("GPU {id}"),
            pci: Some("1002:744c".into()),
            bus: bus.map(Into::into),
            temp: Some(60.0),
            fan,
            source: "nvml",
            ..gpu::Device::default()
        };
        let mut list = vec![Sensor { pci: Some("0000:03:00.0".into()), ..temp("amdgpu", "edge", 55.0) }];
        let gpus = [
            gpu("amd", Some("0000:03:00.0"), None),
            gpu("nv", Some("0000:01:00.0"), Some(gpu::Fan::Percent(40))),
            gpu::Device { id: "igpu".into(), ..gpu::Device::default() },
        ];
        add_gpus(&mut list, &gpus);
        let ids: Vec<&str> = list.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["amdgpu/edge", "gpu:nv/temp", "gpu:nv/fan"]);
        assert_eq!((list[2].kind, list[2].value), (Kind::Duty, 40.0));
        assert_eq!(list[1].chip, "GPU nv");
    }

    #[test]
    fn drives_add_what_no_chip_lists() {
        let t = |v: f32, chip: Option<&str>| disk::Temp {
            value: v,
            high: Some(70.0),
            crit: None,
            source: "smartctl".into(),
            chip: chip.map(Into::into),
        };
        let drive = |id: &str, model: &str, temp| disk::Drive {
            id: id.into(),
            name: id.into(),
            model: model.into(),
            temp,
            ..disk::Drive::default()
        };
        let mut list = vec![temp("hwmon1", "Composite", 38.0)];
        let drives = [
            drive("nvme0n1", "Samsung SSD 980", Some(t(38.0, Some("hwmon1")))),
            drive("sda", "WDC WD40EFRX", Some(t(33.0, None))),
            drive("sdb", "", Some(t(30.0, Some("hwmon9")))),
            drive("vda", "", None),
        ];
        add_drives(&mut list, &drives);
        let ids: Vec<&str> = list.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["hwmon1/Composite", "disk:sda/temp", "disk:sdb/temp"]);
        assert_eq!((list[1].chip.as_str(), list[1].label.as_str(), list[1].high), ("WDC WD40EFRX", "sda", Some(70.0)));
        assert_eq!((list[2].chip.as_str(), list[2].source.as_str()), ("sdb", "smartctl"));
    }
}
