//! macOS: the IOKit calls behind the drives. The registry is read once per
//! tick for the drivers' counts; which drive a volume is on is asked once per
//! device, by walking up the service plane from the volume's `IOMedia` (an
//! APFS volume goes through its container to the partition it lives on).

use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString, c_void};
use std::time::Instant;

use objc2_core_foundation::{CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType, CFUUID, CFUUIDBytes};
use objc2_io_kit::{
    IOBSDNameMatching, IOCFPlugInInterface, IOCreatePlugInInterfaceForService, IODestroyPlugInInterface, IOIteratorNext,
    IOObjectConformsTo, IOObjectRelease, IORegistryEntryCreateCFProperty, IORegistryEntryGetParentEntry, IORegistryEntrySearchCFProperty,
    IOServiceGetMatchingService, IOServiceGetMatchingServices, IOServiceMatching, io_iterator_t, io_object_t,
    kIORegistryIterateRecursively, kIOServicePlane,
};

use super::mac::{self, Raw};
use super::{Drive, Note};

/// `kIOMainPortDefault`.
const MAIN_PORT: u32 = 0;
const DRIVER: &CStr = c"IOBlockStorageDriver";

/// An IOKit object, released when dropped.
struct Obj(io_object_t);

impl Drop for Obj {
    fn drop(&mut self) {
        IOObjectRelease(self.0);
    }
}

/// A property of the entry itself.
fn prop(e: io_object_t, key: &str) -> Option<CFRetained<CFType>> {
    let key = CFString::from_str(key);
    // SAFETY: a live entry and a string key; the default allocator; no options.
    unsafe { IORegistryEntryCreateCFProperty(e, Some(&key), None, 0) }
}

/// A property of the entry or, failing that, of the first child down the
/// service plane that has it.
fn search(e: io_object_t, key: &str) -> Option<CFRetained<CFType>> {
    let key = CFString::from_str(key);
    // SAFETY: a live entry, a plane name that IOKit defines and a string key.
    unsafe {
        IORegistryEntrySearchCFProperty(e, kIOServicePlane.as_ptr().cast_mut().cast(), Some(&key), None, kIORegistryIterateRecursively)
    }
}

fn parent(e: io_object_t) -> Option<Obj> {
    let mut p = 0;
    // SAFETY: a live entry and a plane name that IOKit defines; the parent is
    // written on success and released by `Obj`.
    let ok = unsafe { IORegistryEntryGetParentEntry(e, kIOServicePlane.as_ptr().cast_mut().cast(), &mut p) } == 0;
    (ok && p != 0).then_some(Obj(p))
}

fn is_driver(e: io_object_t) -> bool {
    // SAFETY: a live object and a class name that is a C string.
    unsafe { IOObjectConformsTo(e, DRIVER.as_ptr().cast_mut().cast()) }
}

fn field(dict: Option<&CFType>, key: &str) -> Option<CFRetained<CFType>> {
    let dict = dict?.downcast_ref::<CFDictionary>()?;
    // SAFETY: IOKit's property tables are keyed by strings; the values are
    // only looked at through `downcast_ref`.
    let dict: &CFDictionary<CFString, CFType> = unsafe { dict.cast_unchecked() };
    dict.get(&CFString::from_str(key))
}

fn text(v: Option<CFRetained<CFType>>) -> String {
    v.and_then(|v| v.downcast_ref::<CFString>().map(|s| s.to_string().trim().to_string())).unwrap_or_default()
}

fn number(v: Option<CFRetained<CFType>>) -> Option<u64> {
    v?.downcast_ref::<CFNumber>()?.as_i64().map(|n| n.max(0) as u64)
}

fn flag(v: Option<CFRetained<CFType>>) -> bool {
    v.and_then(|v| v.downcast_ref::<CFBoolean>().map(CFBoolean::as_bool)).unwrap_or(false)
}

/// Every block storage driver; `None` when IOKit cannot be asked.
fn list() -> Option<Vec<Raw>> {
    // SAFETY: a C string.
    let matching = unsafe { IOServiceMatching(DRIVER.as_ptr()) }?;
    let mut iter: io_iterator_t = 0;
    // SAFETY: the dictionary is consumed; the iterator is written on success.
    if unsafe { IOServiceGetMatchingServices(MAIN_PORT, Some(CFRetained::<CFDictionary>::from(&matching)), &mut iter) } != 0 {
        return None;
    }
    let iter = Obj(iter);
    let mut out = Vec::new();
    loop {
        let d = IOIteratorNext(iter.0);
        if d == 0 {
            break;
        }
        out.push(raw(&Obj(d)));
    }
    Some(out)
}

fn raw(driver: &Obj) -> Raw {
    let stats = prop(driver.0, "Statistics");
    let count = |key: &str| number(field(stats.as_deref(), key)).unwrap_or(0);
    let mut r = Raw {
        bsd: text(search(driver.0, "BSD Name")),
        size: number(search(driver.0, "Size")),
        removable: flag(search(driver.0, "Removable")) || flag(search(driver.0, "Ejectable")),
        read: count("Bytes (Read)"),
        write: count("Bytes (Write)"),
        ..Raw::default()
    };
    if let Some(dev) = parent(driver.0) {
        let chars = prop(dev.0, "Device Characteristics");
        let proto = prop(dev.0, "Protocol Characteristics");
        r.product = text(field(chars.as_deref(), "Product Name"));
        r.medium = text(field(chars.as_deref(), "Medium Type"));
        r.interconnect = text(field(proto.as_deref(), "Physical Interconnect"));
        r.location = text(field(proto.as_deref(), "Physical Interconnect Location"));
    }
    r
}

/// The drive (its whole disk's BSD name) that a device is on.
fn owner(bsd: &str) -> Option<String> {
    let mut at = media(bsd)?;
    // A handful of steps in practice; the bound is against a loop.
    for _ in 0..32 {
        at = parent(at.0)?;
        if is_driver(at.0) {
            return Some(text(search(at.0, "BSD Name"))).filter(|s| !s.is_empty());
        }
    }
    None
}

/// The whole disk's `IOMedia` for a BSD name such as `disk0`.
fn media(bsd: &str) -> Option<Obj> {
    let name = CString::new(bsd).ok()?;
    // SAFETY: a C string.
    let matching = unsafe { IOBSDNameMatching(MAIN_PORT, 0, name.as_ptr()) }?;
    // SAFETY: the dictionary is consumed; the service is released by `Obj`.
    let media = unsafe { IOServiceGetMatchingService(MAIN_PORT, Some(CFRetained::<CFDictionary>::from(&matching))) };
    (media != 0).then_some(Obj(media))
}

fn uuid(b: [u8; 16]) -> Option<CFRetained<CFUUID>> {
    let [b0, b1, b2, b3, b4, b5, b6, b7, b8, b9, b10, b11, b12, b13, b14, b15] = b;
    CFUUID::constant_uuid_with_bytes(None, b0, b1, b2, b3, b4, b5, b6, b7, b8, b9, b10, b11, b12, b13, b14, b15)
}

/// `kIONVMeSMARTUserClientTypeID`: the user client of an NVMe drive that
/// says `NVMe SMART Capable` (the IDs are not in a public header; these are
/// the ones smartmontools uses).
const NVME_SMART_CLIENT: [u8; 16] = [0xAA, 0x0F, 0xA6, 0xF9, 0xC2, 0xD6, 0x45, 0x7F, 0xB1, 0x0B, 0x59, 0xA1, 0x32, 0x53, 0x29, 0x2F];
/// `kIONVMeSMARTInterfaceID`.
const NVME_SMART_INTERFACE: [u8; 16] = [0xCC, 0xD1, 0xDB, 0x19, 0xFD, 0x9A, 0x4D, 0xAF, 0xBF, 0x95, 0x12, 0x45, 0x4B, 0x23, 0x0A, 0xB6];
/// `kIOCFPlugInInterfaceID`.
const PLUGIN_INTERFACE: [u8; 16] = [0xC2, 0x44, 0xE8, 0x58, 0x10, 0x9C, 0x11, 0xD4, 0x91, 0xD4, 0x00, 0x50, 0xE4, 0xC6, 0x42, 0x6F];

/// The head of `IONVMeSMARTInterface`: COM's `IUnknown`, a version, then
/// the calls, of which only the first is used (the rest are never read).
#[repr(C)]
struct NvmeSmart {
    _reserved: *mut c_void,
    _query_interface: Option<unsafe extern "C-unwind" fn(*mut c_void, CFUUIDBytes, *mut *mut c_void) -> i32>,
    _add_ref: Option<unsafe extern "C-unwind" fn(*mut c_void) -> u32>,
    release: Option<unsafe extern "C-unwind" fn(*mut c_void) -> u32>,
    _version: u16,
    _revision: u16,
    /// Fills the 512-byte SMART / health information log.
    smart_read_data: Option<unsafe extern "C-unwind" fn(*mut c_void, *mut u8) -> i32>,
}

/// An NVMe drive's SMART / health log (512 bytes), from the user client of
/// the entry above the drive that says `NVMe SMART Capable` (the internal
/// SSD of a Mac, and NVMe drives in enclosures that pass it through). `None`
/// for any other drive.
pub fn nvme_smart(bsd: &str) -> Option<[u8; 512]> {
    let mut at = media(bsd)?;
    let mut found = false;
    for _ in 0..8 {
        at = parent(at.0)?;
        if flag(prop(at.0, "NVMe SMART Capable")) {
            found = true;
            break;
        }
    }
    if !found {
        return None;
    }
    let (client, plugin_id, iface_id) = (uuid(NVME_SMART_CLIENT)?, uuid(PLUGIN_INTERFACE)?, uuid(NVME_SMART_INTERFACE)?);
    let mut plugin: *mut *mut IOCFPlugInInterface = std::ptr::null_mut();
    let mut score = 0;
    // SAFETY: a live service and constant UUIDs; the plug-in is written on
    // success and destroyed below.
    let ok = unsafe { IOCreatePlugInInterfaceForService(at.0, Some(&client), Some(&plugin_id), &mut plugin, &mut score) } == 0;
    if !ok || plugin.is_null() {
        return None;
    }
    let mut iface: *mut *mut NvmeSmart = std::ptr::null_mut();
    // SAFETY: a plug-in IOKit made, whose table starts with `IUnknown`; the
    // interface is written on success and released below.
    let asked =
        unsafe { (**plugin).QueryInterface.map(|q| q(plugin.cast(), iface_id.uuid_bytes(), (&raw mut iface).cast())).unwrap_or(-1) };
    let mut log = [0u8; 512];
    let mut read = false;
    if asked == 0 && !iface.is_null() {
        // SAFETY: the interface the plug-in gave for this UUID, whose table
        // has the layout of `NvmeSmart`; the log is the 512 bytes it fills.
        unsafe {
            read = (**iface).smart_read_data.map(|f| f(iface.cast(), log.as_mut_ptr())) == Some(0);
            if let Some(release) = (**iface).release {
                release(iface.cast());
            }
        }
    }
    // SAFETY: the plug-in made above, not used again.
    unsafe { IODestroyPlugInInterface(plugin) };
    read.then_some(log)
}

/// The device a mount point was mounted from: `/dev/disk3s1s1`.
fn mounted_from(mount: &str) -> Option<String> {
    let path = CString::new(mount).ok()?;
    // SAFETY: a zeroed `statfs` is a valid value to be written over.
    let mut fs: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: a C string path and a buffer of the right type.
    if unsafe { libc::statfs(path.as_ptr(), &mut fs) } != 0 {
        return None;
    }
    // SAFETY: the kernel ends the name with a NUL inside the array.
    let from = unsafe { CStr::from_ptr(fs.f_mntfromname.as_ptr()) };
    Some(from.to_string_lossy().into_owned())
}

#[derive(Default)]
pub struct Reader {
    last: HashMap<String, (u64, u64)>,
    at: Option<Instant>,
    /// The drive under each volume's device, asked once per device and
    /// forgotten when the drives change (a new drive can reuse a name).
    owners: HashMap<String, Option<String>>,
}

impl Reader {
    pub fn read(&mut self) -> (Vec<Drive>, Option<Note>) {
        let Some(raws) = list() else { return (Vec::new(), Some(Note::NoIoKit)) };
        let mut list = mac::drives(raws);
        let now = Instant::now();
        mac::rates(&mut list, &self.last, self.at.map(|t| now.duration_since(t).as_secs_f64()));
        let ids: HashSet<&String> = list.iter().map(|d| &d.id).collect();
        if ids != self.last.keys().collect() {
            self.owners.clear();
        }
        self.last = list.iter().map(|d| (d.id.clone(), (d.read_total.unwrap_or(0), d.write_total.unwrap_or(0)))).collect();
        self.at = Some(now);
        (list, None)
    }

    pub fn drive_of(&mut self, mount: &str) -> Option<String> {
        let from = mounted_from(mount)?;
        let bsd = mac::bsd_of(&from)?;
        self.owners.entry(bsd.to_string()).or_insert_with(|| owner(bsd)).clone()
    }
}
