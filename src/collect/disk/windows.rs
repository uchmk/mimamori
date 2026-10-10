//! Windows: folding the `PhysicalDisk` counters into drives, and reading what
//! `IOCTL_STORAGE_QUERY_PROPERTY` says about each. Pure, so the tests run
//! everywhere; the calls are in `ioctl.rs`.

use std::collections::HashMap;

use super::{Drive, Kind};

/// What a drive says about itself, asked once.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    pub model: String,
    pub bus: Option<&'static str>,
    pub kind: Kind,
    pub size: Option<u64>,
    pub removable: bool,
}

/// `0 C: D:` → disk 0 with its letters. `_Total` is not a disk.
pub fn parse_instance(name: &str) -> Option<(u32, Vec<String>)> {
    let mut words = name.split_whitespace();
    let index = words.next()?.parse().ok()?;
    Some((index, words.filter(|w| w.ends_with(':')).map(str::to_string).collect()))
}

/// `C:\` → `C:`.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn letter(mount: &str) -> Option<String> {
    let mut c = mount.chars();
    let l = c.next().filter(char::is_ascii_alphabetic)?;
    (c.next() == Some(':')).then(|| format!("{}:", l.to_ascii_uppercase()))
}

/// `STORAGE_BUS_TYPE`.
pub fn bus_name(t: u32) -> Option<&'static str> {
    Some(match t {
        1 => "SCSI",
        2 => "ATAPI",
        3 => "ATA",
        4 => "FireWire",
        5 => "SSA",
        6 => "Fibre Channel",
        7 => "USB",
        8 => "RAID",
        9 => "iSCSI",
        10 => "SAS",
        11 => "SATA",
        12 => "SD",
        13 => "MMC",
        14 => "Virtual",
        15 => "File-backed virtual",
        16 => "Storage Spaces",
        17 => "NVMe",
        18 => "SCM",
        19 => "UFS",
        _ => return None,
    })
}

/// The model, bus and removability from a `STORAGE_DEVICE_DESCRIPTOR`:
/// vendor, product offsets at 12 and 16 (0 for none), removable at 10, the
/// bus type at 28.
pub fn describe(buf: &[u8]) -> Option<(String, Option<&'static str>, bool)> {
    let u32_at = |i: usize| Some(u32::from_le_bytes(buf.get(i..i + 4)?.try_into().ok()?));
    let text = |off: u32| -> String {
        let off = off as usize;
        if off == 0 || off >= buf.len() {
            return String::new();
        }
        let end = buf[off..].iter().position(|&b| b == 0).map_or(buf.len(), |n| off + n);
        String::from_utf8_lossy(&buf[off..end]).trim().to_string()
    };
    let (vendor, product) = (text(u32_at(12)?), text(u32_at(16)?));
    let bus = bus_name(u32_at(28)?);
    let removable = *buf.get(10)? != 0;
    // NVMe drives often put "NVMe" as the vendor, or repeat it in the product.
    let model = if vendor.is_empty() || vendor.eq_ignore_ascii_case("nvme") || product.starts_with(&vendor) {
        product
    } else {
        format!("{vendor} {product}")
    };
    Some((model, bus, removable))
}

/// The drives from the three counters (by instance), with what each said
/// about itself. `volumes` holds the drive letters until they are linked to
/// the volumes.
pub fn fold(read: &[(String, f64)], write: &[(String, f64)], idle: &[(String, f64)], info: &HashMap<u32, Info>) -> Vec<Drive> {
    let value = |list: &[(String, f64)], name: &str| list.iter().find(|(n, _)| n == name).map(|(_, v)| *v);
    let mut out: Vec<(u32, Drive)> = read
        .iter()
        .filter_map(|(inst, r)| {
            let (index, letters) = parse_instance(inst)?;
            let i = info.get(&index).cloned().unwrap_or_default();
            let drive = Drive {
                id: index.to_string(),
                name: format!("Disk {index}"),
                model: i.model,
                kind: i.kind,
                bus: i.bus,
                size: i.size,
                removable: i.removable,
                read_rate: r.max(0.0),
                write_rate: value(write, inst).unwrap_or(0.0).max(0.0),
                busy: value(idle, inst).map(|v| (100.0 - v).clamp(0.0, 100.0) as f32),
                volumes: letters,
                ..Drive::default()
            };
            Some((index, drive))
        })
        .collect();
    out.sort_by_key(|(i, _)| *i);
    out.into_iter().map(|(_, d)| d).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_instance_is_a_disk_number_and_its_letters() {
        assert_eq!(parse_instance("0 C: D:"), Some((0, vec!["C:".to_string(), "D:".to_string()])));
        assert_eq!(parse_instance("2"), Some((2, vec![])));
        assert_eq!(parse_instance("_Total"), None);
    }

    #[test]
    fn a_mount_has_a_drive_letter() {
        assert_eq!(letter("C:\\").as_deref(), Some("C:"));
        assert_eq!(letter("d:\\").as_deref(), Some("D:"));
        assert_eq!(letter("\\\\?\\Volume{x}\\"), None);
    }

    #[test]
    fn bus_types_have_names() {
        assert_eq!(bus_name(17), Some("NVMe"));
        assert_eq!(bus_name(11), Some("SATA"));
        assert_eq!(bus_name(7), Some("USB"));
        assert_eq!(bus_name(0), None);
        assert_eq!(bus_name(99), None);
    }

    fn descriptor(vendor: &str, product: &str, bus: u32, removable: bool) -> Vec<u8> {
        let mut b = vec![0u8; 40];
        b[10] = removable as u8;
        b[28..32].copy_from_slice(&bus.to_le_bytes());
        let mut put = |at: usize, s: &str| {
            if s.is_empty() {
                return;
            }
            let off = b.len() as u32;
            b[at..at + 4].copy_from_slice(&off.to_le_bytes());
            b.extend_from_slice(s.as_bytes());
            b.push(0);
        };
        put(12, vendor);
        put(16, product);
        b
    }

    #[test]
    fn a_descriptor_gives_the_model_bus_and_removability() {
        let d = descriptor("", "Samsung SSD 990 PRO 2TB ", 17, false);
        assert_eq!(describe(&d), Some(("Samsung SSD 990 PRO 2TB".to_string(), Some("NVMe"), false)));
        let d = descriptor("NVMe    ", "WD_BLACK SN850X", 17, false);
        assert_eq!(describe(&d).unwrap().0, "WD_BLACK SN850X");
        let d = descriptor("SanDisk ", "Ultra", 7, true);
        assert_eq!(describe(&d), Some(("SanDisk Ultra".to_string(), Some("USB"), true)));
        assert_eq!(describe(&[0u8; 8]), None);
    }

    #[test]
    fn counters_fold_into_drives_by_number() {
        let list = |v: &[(&str, f64)]| v.iter().map(|(n, x)| (n.to_string(), *x)).collect::<Vec<_>>();
        let read = list(&[("1 E:", 10.0), ("0 C: D:", 2048.0), ("_Total", 2058.0)]);
        let write = list(&[("0 C: D:", 512.0)]);
        let idle = list(&[("0 C: D:", 75.0), ("1 E:", 140.0)]);
        let info =
            HashMap::from([(0, Info { model: "Fast".into(), bus: Some("NVMe"), kind: Kind::Ssd, size: Some(1 << 40), removable: false })]);
        let d = fold(&read, &write, &idle, &info);
        assert_eq!(d.len(), 2);
        assert_eq!((d[0].id.as_str(), d[0].name.as_str(), d[0].model.as_str()), ("0", "Disk 0", "Fast"));
        assert_eq!((d[0].read_rate, d[0].write_rate, d[0].busy), (2048.0, 512.0, Some(25.0)));
        assert_eq!(d[0].volumes, ["C:", "D:"]);
        // Idle time over 100 (the counter overshoots) is not negative busy.
        assert_eq!(d[1].busy, Some(0.0));
        assert_eq!((d[1].kind, d[1].write_rate), (Kind::Unknown, 0.0));
    }
}
