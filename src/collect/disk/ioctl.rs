//! Windows: the `PhysicalDisk` performance counters (PDH) for the traffic and
//! how busy each drive is, and `IOCTL_STORAGE_QUERY_PROPERTY` for what each
//! drive is, asked once per drive. The folding is in `windows.rs`.

use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::null_mut;

use ::windows::Win32::Foundation::{CloseHandle, HANDLE};
use ::windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use ::windows::Win32::System::IO::DeviceIoControl;
use ::windows::Win32::System::Ioctl::{
    DEVICE_SEEK_PENALTY_DESCRIPTOR, DISK_GEOMETRY_EX, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, IOCTL_STORAGE_QUERY_PROPERTY,
    PropertyStandardQuery, STORAGE_PROPERTY_ID, STORAGE_PROPERTY_QUERY, StorageDeviceProperty, StorageDeviceSeekPenaltyProperty,
};
use ::windows::Win32::System::Performance::{
    PDH_HCOUNTER, PDH_HQUERY, PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhOpenQueryW,
};
use ::windows::core::{HSTRING, PCWSTR, w};

use super::windows::{Info, describe, fold, parse_instance};
use super::{Drive, Kind};
use crate::collect::pdh::array;

pub struct Reader {
    query: PDH_HQUERY,
    read: PDH_HCOUNTER,
    write: PDH_HCOUNTER,
    idle: Option<PDH_HCOUNTER>,
    /// What each drive said about itself, by disk number.
    info: HashMap<u32, Info>,
}

impl Reader {
    /// `None` when the counters are not there (disabled with `diskperf -n`
    /// or `lodctr /D:PerfDisk`).
    pub fn open() -> Option<Self> {
        let mut query = PDH_HQUERY(null_mut());
        // SAFETY: plain out-parameter calls; the handles are closed in Drop.
        unsafe {
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
                return None;
            }
        }
        let add = |path: PCWSTR| {
            let mut h = PDH_HCOUNTER(null_mut());
            // SAFETY: as above.
            let ok = unsafe { PdhAddEnglishCounterW(query, path, 0, &mut h) } == 0;
            ok.then_some(h)
        };
        let (Some(read), Some(write)) =
            (add(w!("\\PhysicalDisk(*)\\Disk Read Bytes/sec")), add(w!("\\PhysicalDisk(*)\\Disk Write Bytes/sec")))
        else {
            // SAFETY: the query was opened above.
            unsafe { PdhCloseQuery(query) };
            return None;
        };
        let idle = add(w!("\\PhysicalDisk(*)\\% Idle Time"));
        // A rate counter needs a first collection to measure from.
        // SAFETY: as above.
        unsafe { PdhCollectQueryData(query) };
        Some(Self { query, read, write, idle, info: HashMap::new() })
    }

    pub fn read(&mut self) -> Vec<Drive> {
        // SAFETY: the query is open while self lives.
        if unsafe { PdhCollectQueryData(self.query) } != 0 {
            return Vec::new();
        }
        let read = array(self.read);
        for (inst, _) in &read {
            if let Some((index, _)) = parse_instance(inst) {
                self.info.entry(index).or_insert_with(|| query(index));
            }
        }
        let idle = self.idle.map(array).unwrap_or_default();
        fold(&read, &array(self.write), &idle, &self.info)
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        // SAFETY: opened in `open`, closed once here (it frees the counters too).
        unsafe { PdhCloseQuery(self.query) };
    }
}

/// Ask `\\.\PhysicalDriveN` what it is. Opening it with no access rights
/// needs no administrator; what it will not say stays empty.
fn query(index: u32) -> Info {
    let path = HSTRING::from(format!("\\\\.\\PhysicalDrive{index}"));
    // SAFETY: a plain open; the handle is closed below.
    let Ok(h) =
        (unsafe { CreateFileW(&path, 0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None) })
    else {
        return Info::default();
    };
    let mut info = Info::default();
    let mut buf = vec![0u8; 1024];
    if let Some(n) = property(h, StorageDeviceProperty, &mut buf)
        && let Some((model, bus, removable)) = describe(&buf[..n])
    {
        (info.model, info.bus, info.removable) = (model, bus, removable);
    }
    let mut seek = DEVICE_SEEK_PENALTY_DESCRIPTOR::default();
    // SAFETY: the descriptor is a plain struct of its own size.
    let seek_buf = unsafe { std::slice::from_raw_parts_mut((&raw mut seek).cast::<u8>(), size_of::<DEVICE_SEEK_PENALTY_DESCRIPTOR>()) };
    if property(h, StorageDeviceSeekPenaltyProperty, seek_buf).is_some_and(|n| n >= size_of::<DEVICE_SEEK_PENALTY_DESCRIPTOR>()) {
        info.kind = if seek.IncursSeekPenalty { Kind::Hdd } else { Kind::Ssd };
    }
    // DISK_GEOMETRY_EX ends in partition and detection data; room for them.
    let mut geo = vec![0u64; 32];
    let mut got = 0u32;
    // SAFETY: the buffer is 256 bytes, aligned for the struct.
    let ok = unsafe {
        DeviceIoControl(
            h,
            IOCTL_DISK_GET_DRIVE_GEOMETRY_EX,
            None,
            0,
            Some(geo.as_mut_ptr().cast::<c_void>()),
            (geo.len() * 8) as u32,
            Some(&mut got),
            None,
        )
    }
    .is_ok();
    if ok && got as usize >= size_of::<DISK_GEOMETRY_EX>() - 8 {
        // SAFETY: Windows filled at least the fixed part.
        let size = unsafe { (*geo.as_ptr().cast::<DISK_GEOMETRY_EX>()).DiskSize };
        info.size = u64::try_from(size).ok().filter(|&s| s > 0);
    }
    // SAFETY: opened above, closed once.
    unsafe {
        let _ = CloseHandle(h);
    }
    info
}

/// One `IOCTL_STORAGE_QUERY_PROPERTY` into `out`; the bytes written.
fn property(h: HANDLE, id: STORAGE_PROPERTY_ID, out: &mut [u8]) -> Option<usize> {
    let q = STORAGE_PROPERTY_QUERY { PropertyId: id, QueryType: PropertyStandardQuery, ..Default::default() };
    let mut got = 0u32;
    // SAFETY: the query and the buffer outlive the call; their sizes are given.
    unsafe {
        DeviceIoControl(
            h,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some((&raw const q).cast::<c_void>()),
            size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some(out.as_mut_ptr().cast::<c_void>()),
            out.len() as u32,
            Some(&mut got),
            None,
        )
    }
    .ok()?;
    Some(got as usize)
}
