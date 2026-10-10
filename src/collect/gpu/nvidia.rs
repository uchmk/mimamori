//! NVIDIA's management library (NVML), loaded at run time from the driver.
//! Without the driver `open` gives `None` and nothing is listed from it.
//! On Linux it is the source for NVIDIA cards; on Windows only its sensors
//! are laid over the counters' adapters.

use nvml_wrapper::Nvml;
use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use nvml_wrapper::enums::device::UsedGpuMemory;

use super::{Device, Fan, Field, GpuProc, Why};

pub struct Reader {
    nvml: Nvml,
}

impl Reader {
    pub fn open() -> Option<Self> {
        // The unversioned name is only there with the development package.
        #[cfg(target_os = "linux")]
        if let Ok(nvml) = Nvml::builder().lib_path(std::ffi::OsStr::new("libnvidia-ml.so.1")).init() {
            return Some(Self { nvml });
        }
        Nvml::init().ok().map(|nvml| Self { nvml })
    }

    pub fn read(&self) -> Vec<Device> {
        let count = self.nvml.device_count().unwrap_or(0);
        let mut out = Vec::new();
        for i in 0..count {
            let Ok(dev) = self.nvml.device_by_index(i) else { continue };
            let mut d = Device {
                id: format!("nvml:{i}"),
                name: dev.name().unwrap_or_default(),
                vendor: Some("NVIDIA"),
                driver: self.nvml.sys_driver_version().ok().map(|v| format!("nvidia {v}")),
                source: "nvml",
                ..Device::default()
            };
            if let Ok(pci) = dev.pci_info() {
                let id = pci.pci_device_id;
                d.pci = Some(format!("{:04x}:{:04x}", id & 0xffff, id >> 16));
            }
            match dev.utilization_rates() {
                Ok(u) => {
                    d.usage = Some(u.gpu.min(100) as f32);
                    d.engines.push(("Graphics".into(), u.gpu.min(100) as f32));
                    d.engines.push(("Memory".into(), u.memory.min(100) as f32));
                }
                Err(_) => d.why.push((Field::Usage, Why::NoSensor)),
            }
            if let Ok(e) = dev.encoder_utilization() {
                d.engines.push(("Encode".into(), e.utilization.min(100) as f32));
            }
            if let Ok(e) = dev.decoder_utilization() {
                d.engines.push(("Decode".into(), e.utilization.min(100) as f32));
            }
            match dev.memory_info() {
                Ok(m) => (d.vram_used, d.vram_total) = (Some(m.used), Some(m.total)),
                Err(_) => d.why.push((Field::Vram, Why::NoVram)),
            }
            d.temp = dev.temperature(TemperatureSensor::Gpu).ok().map(|t| t as f32);
            d.fan = dev.fan_speed(0).ok().map(Fan::Percent);
            d.power = dev.power_usage().ok().map(|mw| mw as f32 / 1000.0);
            d.clock = dev.clock_info(Clock::Graphics).ok();
            for (f, have) in [(Field::Temp, d.temp.is_some()), (Field::Power, d.power.is_some()), (Field::Clock, d.clock.is_some())] {
                if !have {
                    d.why.push((f, Why::NoSensor));
                }
            }
            let mut procs: Vec<GpuProc> = Vec::new();
            let lists = [dev.running_graphics_processes(), dev.running_compute_processes()];
            for p in lists.into_iter().flatten().flatten() {
                let mem = match p.used_gpu_memory {
                    UsedGpuMemory::Used(b) => Some(b),
                    UsedGpuMemory::Unavailable => None,
                };
                match procs.iter_mut().find(|q| q.pid == p.pid) {
                    Some(q) => q.mem = q.mem.max(mem),
                    None => procs.push(GpuProc { pid: p.pid, usage: None, mem }),
                }
            }
            procs.sort_by(|a, b| b.mem.cmp(&a.mem).then(a.pid.cmp(&b.pid)));
            d.procs = procs;
            out.push(d);
        }
        out
    }
}
