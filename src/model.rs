//! What the screens draw from: the latest readings and the last minute of
//! history, folded in from the sampler's messages.

use std::collections::{HashMap, VecDeque};

use crate::collect::{Mem, Msg, Proc, Static};

/// Samples kept for the graphs and the heatmap: a minute at the default
/// interval.
pub const HISTORY: usize = 60;

/// The newest [`HISTORY`] values, oldest first.
#[derive(Clone, Debug, Default)]
pub struct Ring(VecDeque<f32>);

impl Ring {
    pub fn push(&mut self, v: f32) {
        if self.0.len() == HISTORY {
            self.0.pop_front();
        }
        self.0.push_back(v);
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = f32> + '_ {
        self.0.iter().copied()
    }

    pub fn last(&self) -> Option<f32> {
        self.0.back().copied()
    }

    pub fn max(&self) -> f32 {
        self.0.iter().copied().fold(0.0, f32::max)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

#[derive(Default)]
pub struct Model {
    pub st: Option<Static>,
    /// Per logical CPU, percent.
    pub usage: Vec<f32>,
    pub freq: Vec<u64>,
    pub total: f32,
    pub load: Option<(f64, f64, f64)>,
    pub cpu_hist: Ring,
    /// One ring per logical CPU: the heatmap's rows.
    pub threads: Vec<Ring>,
    pub mem: Mem,
    /// Used RAM and swap in percent of their totals.
    pub mem_hist: Ring,
    pub swap_hist: Ring,
    pub procs: Vec<Proc>,
    pub total_threads: Option<u64>,
    /// CPU and resident bytes per live process, for the detail column.
    pub proc_hist: HashMap<u32, (Ring, Ring)>,
    /// CPU readings so far.
    pub samples: u64,
}

fn pct(part: u64, whole: u64) -> f32 {
    if whole == 0 { 0.0 } else { (part as f64 * 100.0 / whole as f64) as f32 }
}

impl Model {
    pub fn apply(&mut self, msg: Msg) {
        match msg {
            Msg::Static(st) => self.st = Some(*st),
            Msg::Cpu { usage, freq_mhz, total, load } => {
                if self.threads.len() != usage.len() {
                    self.threads = vec![Ring::default(); usage.len()];
                }
                for (r, &u) in self.threads.iter_mut().zip(&usage) {
                    r.push(u);
                }
                self.cpu_hist.push(total);
                self.usage = usage;
                self.freq = freq_mhz;
                self.total = total;
                self.load = load;
                self.samples += 1;
            }
            Msg::Mem(m) => {
                self.mem_hist.push(pct(m.used, m.total));
                self.swap_hist.push(pct(m.swap_used, m.swap_total));
                self.mem = m;
            }
            Msg::Procs { procs, total_threads } => {
                let mut hist = std::mem::take(&mut self.proc_hist);
                hist.retain(|pid, _| procs.iter().any(|p| p.pid == *pid));
                for p in &procs {
                    let (c, m) = hist.entry(p.pid).or_default();
                    c.push(p.cpu);
                    m.push(p.mem as f32);
                }
                self.proc_hist = hist;
                self.procs = procs;
                self.total_threads = total_threads;
            }
        }
    }

    pub fn mem_pct(&self) -> f32 {
        pct(self.mem.used, self.mem.total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, cpu: f32) -> Proc {
        Proc {
            pid,
            ppid: None,
            name: format!("p{pid}"),
            cpu,
            mem: 1024,
            threads: Some(1),
            user: None,
            cmd: String::new(),
            exe: None,
            start: 0,
            status: "Run".into(),
        }
    }

    #[test]
    fn the_ring_keeps_the_newest_minute() {
        let mut r = Ring::default();
        for i in 0..100 {
            r.push(i as f32);
        }
        assert_eq!(r.len(), HISTORY);
        assert_eq!(r.iter().next(), Some(40.0));
        assert_eq!(r.last(), Some(99.0));
        assert_eq!(r.max(), 99.0);
    }

    #[test]
    fn readings_fold_in_and_dead_processes_drop() {
        let mut m = Model::default();
        m.apply(Msg::Cpu { usage: vec![10.0, 30.0], freq_mhz: vec![3000, 3000], total: 20.0, load: None });
        assert_eq!(m.threads.len(), 2);
        assert_eq!(m.cpu_hist.last(), Some(20.0));
        m.apply(Msg::Mem(Mem { total: 400, used: 100, ..Mem::default() }));
        assert_eq!(m.mem_pct(), 25.0);
        m.apply(Msg::Procs { procs: vec![proc(1, 5.0), proc(2, 7.0)], total_threads: None });
        m.apply(Msg::Procs { procs: vec![proc(2, 9.0)], total_threads: None });
        assert!(!m.proc_hist.contains_key(&1));
        assert_eq!(m.proc_hist[&2].0.len(), 2);
    }
}
