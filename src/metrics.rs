//! `[agent] metrics`: what the agent has read, in Prometheus' text format
//! (`/metrics`). The text is made from the agent's own [`Model`] each time
//! it is asked for, so nothing is spent while nobody scrapes.

use std::fmt::Write as _;

use crate::collect::sensors::Kind;
use crate::model::Model;

/// A `# HELP` and `# TYPE` header, written once per metric.
fn head(out: &mut String, name: &str, kind: &str, help: &str) {
    let _ = writeln!(out, "# HELP {name} {help}\n# TYPE {name} {kind}");
}

/// A label value with `\`, `"` and newlines escaped.
fn esc(v: &str) -> String {
    v.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

/// A sample; `labels` are `(name, value)` pairs.
fn sample(out: &mut String, name: &str, labels: &[(&str, &str)], v: f64) {
    out.push_str(name);
    if !labels.is_empty() {
        out.push('{');
        for (i, (k, val)) in labels.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let _ = write!(out, "{k}=\"{}\"", esc(val));
        }
        out.push('}');
    }
    // `f64`'s Display never gives an exponent, and NaN and infinity are not made.
    let v = if v.is_finite() { v } else { 0.0 };
    let _ = writeln!(out, " {v}");
}

/// The whole page.
pub fn render(m: &Model) -> String {
    let mut o = String::new();
    head(&mut o, "yagura_info", "gauge", "The yagura that answers, by version.");
    sample(&mut o, "yagura_info", &[("version", env!("CARGO_PKG_VERSION"))], 1.0);

    head(&mut o, "yagura_cpu_usage_percent", "gauge", "CPU use over all logical CPUs.");
    sample(&mut o, "yagura_cpu_usage_percent", &[], f64::from(m.total));
    head(&mut o, "yagura_cpu_core_usage_percent", "gauge", "CPU use of each logical CPU.");
    for (i, u) in m.usage.iter().enumerate() {
        sample(&mut o, "yagura_cpu_core_usage_percent", &[("cpu", &i.to_string())], f64::from(*u));
    }
    if let Some((a, b, c)) = m.load {
        head(&mut o, "yagura_load_average", "gauge", "The 1, 5 and 15 minute load average.");
        for (w, v) in [("1m", a), ("5m", b), ("15m", c)] {
            sample(&mut o, "yagura_load_average", &[("window", w)], v);
        }
    }

    if let Some(s) = &m.session {
        head(&mut o, "yagura_logged_in_users", "gauge", "Logins on the machine.");
        sample(&mut o, "yagura_logged_in_users", &[], s.users.len() as f64);
    }

    for (name, help, v) in [
        ("yagura_memory_total_bytes", "Physical memory.", m.mem.total),
        ("yagura_memory_used_bytes", "Physical memory in use.", m.mem.used),
        ("yagura_memory_available_bytes", "Physical memory a program could still have.", m.mem.available),
        ("yagura_swap_total_bytes", "Swap or page file.", m.mem.swap_total),
        ("yagura_swap_used_bytes", "Swap or page file in use.", m.mem.swap_used),
    ] {
        head(&mut o, name, "gauge", help);
        sample(&mut o, name, &[], v as f64);
    }

    let nics: Vec<_> = m.nics.iter().filter(|n| n.up).collect();
    if !nics.is_empty() {
        head(&mut o, "yagura_network_receive_bytes_total", "counter", "Bytes received since the interface came up.");
        for n in &nics {
            sample(&mut o, "yagura_network_receive_bytes_total", &[("interface", &n.name)], n.rx_total as f64);
        }
        head(&mut o, "yagura_network_transmit_bytes_total", "counter", "Bytes sent since the interface came up.");
        for n in &nics {
            sample(&mut o, "yagura_network_transmit_bytes_total", &[("interface", &n.name)], n.tx_total as f64);
        }
        head(&mut o, "yagura_network_receive_bytes_per_second", "gauge", "Receive rate since the last reading.");
        for n in &nics {
            sample(&mut o, "yagura_network_receive_bytes_per_second", &[("interface", &n.name)], n.rx_rate);
        }
        head(&mut o, "yagura_network_transmit_bytes_per_second", "gauge", "Send rate since the last reading.");
        for n in &nics {
            sample(&mut o, "yagura_network_transmit_bytes_per_second", &[("interface", &n.name)], n.tx_rate);
        }
    }

    if !m.disks.volumes.is_empty() {
        head(&mut o, "yagura_volume_size_bytes", "gauge", "Size of a volume.");
        for v in &m.disks.volumes {
            sample(&mut o, "yagura_volume_size_bytes", &[("mount", &v.mount), ("fs", &v.fs)], v.total as f64);
        }
        head(&mut o, "yagura_volume_available_bytes", "gauge", "Free space of a volume.");
        for v in &m.disks.volumes {
            sample(&mut o, "yagura_volume_available_bytes", &[("mount", &v.mount), ("fs", &v.fs)], v.avail as f64);
        }
    }
    let (read, write) = m.disks.rates();
    head(&mut o, "yagura_disk_read_bytes_per_second", "gauge", "Read rate over all drives.");
    sample(&mut o, "yagura_disk_read_bytes_per_second", &[], read);
    head(&mut o, "yagura_disk_write_bytes_per_second", "gauge", "Write rate over all drives.");
    sample(&mut o, "yagura_disk_write_bytes_per_second", &[], write);

    let temps: Vec<_> = m.sensors.list.iter().filter(|s| s.kind == Kind::Temp).collect();
    if !temps.is_empty() {
        head(&mut o, "yagura_temperature_celsius", "gauge", "A temperature sensor.");
        for s in temps {
            sample(&mut o, "yagura_temperature_celsius", &[("chip", &s.chip), ("sensor", &s.label)], f64::from(s.value));
        }
    }
    let fans: Vec<_> = m.sensors.list.iter().filter(|s| s.kind == Kind::Fan).collect();
    if !fans.is_empty() {
        head(&mut o, "yagura_fan_rpm", "gauge", "A fan's speed.");
        for s in fans {
            sample(&mut o, "yagura_fan_rpm", &[("chip", &s.chip), ("fan", &s.label)], f64::from(s.value));
        }
    }

    let gpus: Vec<(usize, &crate::collect::gpu::Device)> = m.gpus.iter().enumerate().collect();
    if !gpus.is_empty() {
        let name = |d: &crate::collect::gpu::Device| if d.name.is_empty() { d.id.clone() } else { d.name.clone() };
        head(&mut o, "yagura_gpu_usage_percent", "gauge", "The busiest engine of a GPU or NPU.");
        for (i, d) in &gpus {
            if let Some(u) = d.usage {
                sample(&mut o, "yagura_gpu_usage_percent", &[("gpu", &i.to_string()), ("name", &name(d))], f64::from(u));
            }
        }
        head(&mut o, "yagura_gpu_memory_used_bytes", "gauge", "Dedicated memory in use.");
        for (i, d) in &gpus {
            if let Some(u) = d.vram_used {
                sample(&mut o, "yagura_gpu_memory_used_bytes", &[("gpu", &i.to_string()), ("name", &name(d))], u as f64);
            }
        }
        head(&mut o, "yagura_gpu_temperature_celsius", "gauge", "A GPU's temperature.");
        for (i, d) in &gpus {
            if let Some(t) = d.temp {
                sample(&mut o, "yagura_gpu_temperature_celsius", &[("gpu", &i.to_string()), ("name", &name(d))], f64::from(t));
            }
        }
    }

    if let Some(b) = m.battery {
        head(&mut o, "yagura_battery_charge_percent", "gauge", "Battery charge.");
        sample(&mut o, "yagura_battery_charge_percent", &[], f64::from(b.pct));
        head(&mut o, "yagura_battery_plugged", "gauge", "1 on mains power.");
        sample(&mut o, "yagura_battery_plugged", &[], f64::from(u8::from(b.plugged)));
    }

    if !m.procs.is_empty() {
        head(&mut o, "yagura_processes", "gauge", "Processes running (read while a viewer watches procs).");
        sample(&mut o, "yagura_processes", &[], m.procs.len() as f64);
    }
    if let Some(c) = &m.containers {
        head(&mut o, "yagura_container_running", "gauge", "1 while a container runs (read while a viewer watches containers).");
        for k in &c.list {
            sample(&mut o, "yagura_container_running", &[("name", &k.name), ("image", &k.image)], f64::from(u8::from(k.state.live())));
        }
        head(&mut o, "yagura_container_cpu_percent", "gauge", "A container's CPU use, 100 per logical CPU.");
        for k in &c.list {
            if let Some(v) = k.cpu {
                sample(&mut o, "yagura_container_cpu_percent", &[("name", &k.name)], f64::from(v));
            }
        }
        head(&mut o, "yagura_container_memory_bytes", "gauge", "A container's memory without the file cache.");
        for k in &c.list {
            if let Some(v) = k.mem {
                sample(&mut o, "yagura_container_memory_bytes", &[("name", &k.name)], v as f64);
            }
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect::net::Nic;

    #[test]
    fn labels_are_escaped() {
        let mut o = String::new();
        sample(&mut o, "x", &[("a", "q\"\\\n")], 1.0);
        assert_eq!(o, "x{a=\"q\\\"\\\\\\n\"} 1\n");
    }

    #[test]
    fn the_unsayable_is_not_a_number() {
        let mut o = String::new();
        sample(&mut o, "x", &[], f64::NAN);
        assert_eq!(o, "x 0\n");
    }

    #[test]
    fn a_page_has_a_header_before_each_metric_and_no_blank_line() {
        let m = Model {
            total: 12.5,
            usage: vec![10.0, 15.0],
            nics: vec![Nic { name: "eth0".into(), up: true, rx_total: 7, ..Nic::default() }],
            ..Model::default()
        };
        let page = render(&m);
        assert!(page.contains("# TYPE yagura_cpu_usage_percent gauge\nyagura_cpu_usage_percent 12.5\n"), "{page}");
        assert!(page.contains("yagura_cpu_core_usage_percent{cpu=\"1\"} 15\n"));
        assert!(page.contains("yagura_network_receive_bytes_total{interface=\"eth0\"} 7\n"));
        assert!(!page.contains("\n\n"));
        // Every sample's metric was announced first.
        let mut seen = std::collections::HashSet::new();
        for l in page.lines() {
            if let Some(t) = l.strip_prefix("# TYPE ") {
                seen.insert(t.split(' ').next().unwrap().to_string());
            } else if !l.starts_with('#') {
                let name = l.split(['{', ' ']).next().unwrap();
                assert!(seen.contains(name), "{name} has no TYPE");
            }
        }
    }

    #[test]
    fn what_is_not_read_is_left_out() {
        let page = render(&Model::default());
        assert!(
            !page.contains("yagura_battery")
                && !page.contains("yagura_container")
                && !page.contains("yagura_load_average")
                && !page.contains("yagura_logged_in")
        );
    }
}
