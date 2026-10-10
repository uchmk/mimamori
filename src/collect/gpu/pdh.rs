//! Windows: the GPU performance counters (PDH) and the adapters (DXGI).
//! Opened once on the sampler thread; each reading collects the query and
//! hands the counters to [`fold`].

use std::ptr::null_mut;

use ::windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1};
use ::windows::Win32::System::Performance::{
    PDH_CSTATUS_NEW_DATA, PDH_FMT, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
};
use ::windows::core::{PCWSTR, w};

use super::windows::{Adapter, fold, luid_name};
use super::{Device, vendor_name};

/// Doubles, not capped at 100 (an engine summed over processes can pass it
/// for a moment; `fold` caps after summing).
const FMT: PDH_FMT = PDH_FMT(PDH_FMT_DOUBLE.0 | 0x8000);

pub struct Reader {
    query: PDH_HQUERY,
    engines: PDH_HCOUNTER,
    dedicated: Option<PDH_HCOUNTER>,
    shared: Option<PDH_HCOUNTER>,
    proc_mem: Option<PDH_HCOUNTER>,
    adapters: Vec<Adapter>,
}

impl Reader {
    /// `None` when the counters are not there (before Windows 10 1709, or
    /// with the performance counters disabled).
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
        let Some(engines) = add(w!("\\GPU Engine(*)\\Utilization Percentage")) else {
            // SAFETY: the query was opened above.
            unsafe { PdhCloseQuery(query) };
            return None;
        };
        let dedicated = add(w!("\\GPU Adapter Memory(*)\\Dedicated Usage"));
        let shared = add(w!("\\GPU Adapter Memory(*)\\Shared Usage"));
        let proc_mem = add(w!("\\GPU Process Memory(*)\\Dedicated Usage"));
        // A rate counter needs a first collection to measure from.
        // SAFETY: as above.
        unsafe { PdhCollectQueryData(query) };
        Some(Self { query, engines, dedicated, shared, proc_mem, adapters: adapters() })
    }

    pub fn read(&mut self) -> Vec<Device> {
        // SAFETY: the query is open while self lives.
        if unsafe { PdhCollectQueryData(self.query) } != 0 {
            return Vec::new();
        }
        let opt = |h: Option<PDH_HCOUNTER>| h.map(array).unwrap_or_default();
        fold(&self.adapters, &array(self.engines), &opt(self.dedicated), &opt(self.shared), &opt(self.proc_mem))
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        // SAFETY: opened in `open`, closed once here (it frees the counters too).
        unsafe { PdhCloseQuery(self.query) };
    }
}

/// Every instance of a wildcard counter with its value.
fn array(h: PDH_HCOUNTER) -> Vec<(String, f64)> {
    // Instances come and go between the two calls; a few tries settle it.
    for _ in 0..3 {
        let (mut size, mut count) = (0u32, 0u32);
        // SAFETY: asking for the size only.
        let r = unsafe { PdhGetFormattedCounterArrayW(h, FMT, &mut size, &mut count, None) };
        if r != PDH_MORE_DATA || size == 0 {
            return Vec::new();
        }
        // u64s so the items (which hold an f64) are aligned.
        let mut buf = vec![0u64; (size as usize).div_ceil(8)];
        let items = buf.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        // SAFETY: the buffer is `size` bytes or more, as PDH asked.
        let r = unsafe { PdhGetFormattedCounterArrayW(h, FMT, &mut size, &mut count, Some(items)) };
        if r == PDH_MORE_DATA {
            continue;
        }
        if r != 0 {
            return Vec::new();
        }
        // SAFETY: PDH wrote `count` items at the start of the buffer; their
        // names point into the rest of it, which lives until we return.
        let items = unsafe { std::slice::from_raw_parts(items, count as usize) };
        return items
            .iter()
            .filter(|i| i.FmtValue.CStatus == 0 || i.FmtValue.CStatus == PDH_CSTATUS_NEW_DATA)
            .filter_map(|i| {
                // SAFETY: as above; the value was asked for as a double.
                let name = unsafe { i.szName.to_string() }.ok()?;
                Some((name, unsafe { i.FmtValue.Anonymous.doubleValue }))
            })
            .collect();
    }
    Vec::new()
}

/// The adapters DXGI lists, read once (they do not change while running,
/// short of an eGPU being plugged in).
fn adapters() -> Vec<Adapter> {
    // SAFETY: DXGI's factory needs no COM initialisation.
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else { return Vec::new() };
    let mut out: Vec<Adapter> = Vec::new();
    for i in 0u32.. {
        // SAFETY: enumeration ends with an error past the last adapter.
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(i) }) else { break };
        // SAFETY: as above.
        let Ok(d) = (unsafe { adapter.GetDesc1() }) else { continue };
        let luid = luid_name(d.AdapterLuid.HighPart, d.AdapterLuid.LowPart);
        if out.iter().any(|a| a.luid == luid) {
            continue;
        }
        let len = d.Description.iter().position(|&c| c == 0).unwrap_or(d.Description.len());
        out.push(Adapter {
            luid,
            name: String::from_utf16_lossy(&d.Description[..len]).trim().to_string(),
            vendor: vendor_name(d.VendorId),
            pci: Some(format!("{:04x}:{:04x}", d.VendorId, d.DeviceId)),
            dedicated: d.DedicatedVideoMemory as u64,
            shared: d.SharedSystemMemory as u64,
            software: d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0,
        });
    }
    out
}
