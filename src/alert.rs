//! Threshold alerts: `[[alert]]` rules in config.toml, checked against this
//! machine's readings, and `[notify]`, how a fired one reaches a person (the
//! system's notification, a sound, a webhook; `ito-notify`, the same words as
//! tsumugi's `[notify]`).
//!
//! ```toml
//! [notify]
//! system = true
//! sound = false
//! sound_name = "alert"          # chime, low, alert or default
//! focus_mode = true             # Windows: no sound while focus mode is on
//! webhook = "https://ntfy.sh/my-topic"
//! webhook_format = "ntfy"       # ntfy, slack, discord or json
//!
//! [[alert]]
//! metric = "cpu"                # cpu, mem, swap, gpu, temp, disk, battery
//! above = 90                    # or `below = 15`
//! for = 30                      # seconds it has to hold (default 10)
//! name = "CPU is busy"          # optional
//!
//! [[alert]]
//! metric = "service"            # fires when the service is stopped or failed
//! service = "nginx"             # its name as svc shows it (case does not matter)
//! ```
//!
//! A rule fires once when its condition has held for `for` seconds, and again
//! only after it has stopped holding.

use std::time::{Duration, Instant};

use crate::collect::svc;
use crate::i18n::{tr, trf};
use crate::model::Model;

/// What a rule watches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metric {
    /// Total CPU use, percent.
    Cpu,
    /// RAM in use, percent.
    Mem,
    /// Swap in use, percent.
    Swap,
    /// The busiest GPU, percent.
    Gpu,
    /// The hottest temperature, °C.
    Temp,
    /// The fullest volume, percent used.
    Disk,
    /// Charge, percent.
    Battery,
    /// A named service being stopped or failed (see `Rule::service`).
    Service,
}

pub const METRICS: [(&str, Metric); 8] = [
    ("cpu", Metric::Cpu),
    ("mem", Metric::Mem),
    ("swap", Metric::Swap),
    ("gpu", Metric::Gpu),
    ("temp", Metric::Temp),
    ("disk", Metric::Disk),
    ("battery", Metric::Battery),
    ("service", Metric::Service),
];

impl Metric {
    /// The language table's key for its name.
    pub fn key(self) -> &'static str {
        match self {
            Metric::Cpu => "alert.cpu",
            Metric::Mem => "alert.mem",
            Metric::Swap => "alert.swap",
            Metric::Gpu => "alert.gpu",
            Metric::Temp => "alert.temp",
            Metric::Disk => "alert.disk",
            Metric::Battery => "alert.battery",
            Metric::Service => "alert.service",
        }
    }

    pub fn unit(self) -> &'static str {
        if self == Metric::Temp { "°C" } else { "%" }
    }

    /// The reading now, when this machine has one.
    pub fn read(self, m: &Model) -> Option<f32> {
        let pct = |used: u64, total: u64| (total > 0).then(|| used as f32 * 100.0 / total as f32);
        match self {
            Metric::Cpu => (m.samples > 0).then_some(m.total),
            Metric::Mem => pct(m.mem.used, m.mem.total),
            Metric::Swap => pct(m.mem.swap_used, m.mem.swap_total),
            Metric::Gpu => m.gpus.iter().filter_map(|g| g.usage).max_by(f32::total_cmp),
            Metric::Temp => m.sensors.hottest().map(|s| s.value),
            Metric::Disk => m
                .disks
                .volumes
                .iter()
                .filter(|v| v.total > 0 && !v.read_only)
                .map(|v| (v.total - v.avail.min(v.total)) as f32 * 100.0 / v.total as f32)
                .max_by(f32::total_cmp),
            Metric::Battery => m.battery.map(|b| b.pct),
            // Needs the rule's service name: `Rule::read`.
            Metric::Service => None,
        }
    }
}

/// One `[[alert]]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub metric: Metric,
    pub limit: f32,
    /// True: fires over the limit; false: under it.
    pub above: bool,
    /// Seconds the condition has to hold.
    pub secs: u64,
    pub name: Option<String>,
    /// For `Metric::Service`: the service to watch.
    pub service: Option<String>,
}

impl Rule {
    /// The reading now: for a service, 1 while it is stopped or failed (0
    /// while it runs; nothing when it is not listed or is changing state).
    pub fn read(&self, m: &Model) -> Option<f32> {
        let Some(want) = self.service.as_deref() else { return self.metric.read(m) };
        let s = m.services.list.iter().find(|s| !s.driver && s.name.eq_ignore_ascii_case(want))?;
        match s.state {
            svc::State::Stopped | svc::State::Failed => Some(1.0),
            svc::State::Running | svc::State::Paused => Some(0.0),
            svc::State::Starting | svc::State::Stopping | svc::State::Unknown => None,
        }
    }

    fn holds(&self, v: f32) -> bool {
        if self.above { v > self.limit } else { v < self.limit }
    }
}

/// `[notify]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Notify {
    /// The system's notification.
    pub system: bool,
    /// A sound of the system's.
    pub sound: bool,
    pub sound_name: String,
    /// Windows: no sound while focus mode (do not disturb) or a full-screen
    /// program is on.
    pub focus_mode: bool,
    /// Where to send it too; empty: nowhere.
    pub webhook: String,
    pub webhook_format: String,
}

impl Default for Notify {
    fn default() -> Self {
        Self {
            system: true,
            sound: false,
            sound_name: "alert".into(),
            focus_mode: true,
            webhook: String::new(),
            webhook_format: "ntfy".into(),
        }
    }
}

fn number(v: &toml::Value) -> Option<f64> {
    v.as_float().or_else(|| v.as_integer().map(|i| i as f64))
}

/// `[notify]`, warning about what it cannot read.
pub fn parse_notify(v: &toml::Value, warn: &mut Vec<String>) -> Notify {
    let mut n = Notify::default();
    let Some(t) = v.as_table() else {
        warn.push("config.toml: `notify` must be a table".into());
        return n;
    };
    for (k, v) in t {
        let bool_of = |warn: &mut Vec<String>| {
            let b = v.as_bool();
            if b.is_none() {
                warn.push(format!("config.toml: [notify] `{k}` is true or false"));
            }
            b
        };
        let str_of = |warn: &mut Vec<String>| {
            let s = v.as_str().map(|s| s.trim().to_owned());
            if s.is_none() {
                warn.push(format!("config.toml: [notify] `{k}` must be a string"));
            }
            s
        };
        match k.as_str() {
            "system" => n.system = bool_of(warn).unwrap_or(n.system),
            "sound" => n.sound = bool_of(warn).unwrap_or(n.sound),
            "focus_mode" => n.focus_mode = bool_of(warn).unwrap_or(n.focus_mode),
            "sound_name" => match str_of(warn) {
                Some(s) if ito_notify::SOUNDS.contains(&s.as_str()) => n.sound_name = s,
                Some(s) => warn.push(format!("config.toml: [notify] `sound_name` `{s}` is not {}", ito_notify::SOUNDS.join(", "))),
                None => {}
            },
            "webhook" => {
                if let Some(s) = str_of(warn) {
                    // Kept, so the settings show it, but not sent to.
                    if !s.is_empty() && !ito_notify::is_http(&s) {
                        warn.push(format!("config.toml: [notify] `webhook` `{s}` is not an http(s) address"));
                    }
                    n.webhook = s;
                }
            }
            "webhook_format" => match str_of(warn) {
                Some(s) if ito_notify::FORMATS.contains(&s.as_str()) => n.webhook_format = s,
                Some(s) => warn.push(format!("config.toml: [notify] `webhook_format` `{s}` is not {}", ito_notify::FORMATS.join(", "))),
                None => {}
            },
            other => warn.push(format!("config.toml: unknown setting `notify.{other}`")),
        }
    }
    n
}

/// `[[alert]]`: a rule that cannot be read is warned about and left out.
pub fn parse_rules(v: &toml::Value, warn: &mut Vec<String>) -> Vec<Rule> {
    let Some(list) = v.as_array() else {
        warn.push("config.toml: `alert` must be [[alert]] tables".into());
        return Vec::new();
    };
    let mut out = Vec::new();
    for (i, x) in list.iter().enumerate() {
        let file = format!("config.toml [[alert]] {}", i + 1);
        let Some(t) = x.as_table() else {
            warn.push(format!("{file} must be a table"));
            continue;
        };
        for k in t.keys().filter(|k| !["metric", "above", "below", "for", "name", "service"].contains(&k.as_str())) {
            warn.push(format!("{file}: unknown setting `{k}`"));
        }
        let Some(metric) = t.get("metric").and_then(|m| m.as_str()).and_then(|w| METRICS.iter().find(|(n, _)| *n == w)).map(|(_, m)| *m)
        else {
            let words = METRICS.map(|(w, _)| w).join(", ");
            warn.push(format!("{file}: `metric` is one of {words}"));
            continue;
        };
        let service = t.get("service").and_then(|n| n.as_str()).map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned);
        let (above, below) = (t.get("above").and_then(number), t.get("below").and_then(number));
        let (limit, is_above) = if metric == Metric::Service {
            if service.is_none() || above.is_some() || below.is_some() {
                warn.push(format!("{file}: `service` needs `service = \"name\"` and no `above` or `below`"));
                continue;
            }
            (0.5, true)
        } else {
            if service.is_some() {
                warn.push(format!("{file}: `service` is for `metric = \"service\"`"));
                continue;
            }
            match (above, below) {
                (Some(a), None) => (a as f32, true),
                (None, Some(b)) => (b as f32, false),
                _ => {
                    warn.push(format!("{file}: needs one number, `above` or `below`"));
                    continue;
                }
            }
        };
        let secs = match t.get("for").map(|v| v.as_integer().filter(|s| (0..=86_400).contains(s))) {
            None => 10,
            Some(Some(s)) => s as u64,
            Some(None) => {
                warn.push(format!("{file}: `for` is whole seconds from 0 to 86400"));
                continue;
            }
        };
        let name = t.get("name").and_then(|n| n.as_str()).map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned);
        out.push(Rule { metric, limit, above: is_above, secs, name, service });
    }
    out
}

/// A rule that has fired.
#[derive(Clone, Debug, PartialEq)]
pub struct Fired {
    pub rule: Rule,
    pub value: f32,
}

impl Fired {
    /// The notification's title and body, on `host`.
    pub fn words(&self, host: &str) -> (String, String) {
        let r = &self.rule;
        if let Some(svc) = &r.service {
            let title = r.name.clone().unwrap_or_else(|| trf("alert.service_down", &[svc]));
            let body = if host.is_empty() { trf("alert.service_now", &[svc]) } else { trf("alert.service_now_on", &[svc, host]) };
            return (title, body);
        }
        let label = tr(r.metric.key());
        let limit = format!("{}{}", r.limit, r.metric.unit());
        let title = r.name.clone().unwrap_or_else(|| trf(if r.above { "alert.above" } else { "alert.below" }, &[label, &limit]));
        let now = format!("{:.0}{}", self.value, r.metric.unit());
        let body = if host.is_empty() { trf("alert.now", &[label, &now]) } else { trf("alert.now_on", &[label, &now, host]) };
        (title, body)
    }
}

/// What the rules have seen so far: since when each holds, and whether it has
/// said so.
#[derive(Default)]
pub struct Tracker {
    rules: Vec<Rule>,
    state: Vec<(Option<Instant>, bool)>,
}

impl Tracker {
    /// Nothing is being watched or remembered.
    pub fn is_idle(&self) -> bool {
        self.rules.is_empty()
    }

    /// Look at the readings at `now`; the rules that have just fired. A
    /// changed list of rules starts over.
    pub fn step(&mut self, rules: &[Rule], now: Instant, read: impl Fn(&Rule) -> Option<f32>) -> Vec<Fired> {
        if self.rules != rules {
            self.rules = rules.to_vec();
            self.state = vec![(None, false); rules.len()];
        }
        let mut out = Vec::new();
        for (rule, (since, told)) in rules.iter().zip(&mut self.state) {
            match read(rule).filter(|v| rule.holds(*v)) {
                Some(value) => {
                    let start = *since.get_or_insert(now);
                    if !*told && now.duration_since(start) >= Duration::from_secs(rule.secs) {
                        *told = true;
                        out.push(Fired { rule: rule.clone(), value });
                    }
                }
                None => (*since, *told) = (None, false),
            }
        }
        out
    }
}

/// Tell a person: the system's notification, a sound and the webhook, as
/// `[notify]` asks.
pub fn tell(n: &Notify, title: &str, body: &str) {
    if n.system {
        ito_notify::toast::show_with_icon(ito_notify::toast::App { id: "uchmk.yagura", name: "yagura" }, crate::icon::rgba, title, body);
    }
    if n.sound && !(n.focus_mode && ito_notify::sound::quiet_time()) {
        ito_notify::sound::play(&n.sound_name);
    }
    if ito_notify::is_http(&n.webhook) {
        ito_notify::webhook::send(
            n.webhook.clone(),
            n.webhook_format.clone(),
            ito_notify::webhook::Level::Alert,
            title.to_owned(),
            body.to_owned(),
            String::new(),
        );
    }
}

impl Notify {
    /// As the settings' Notifications page shows it: one event, the alert,
    /// and the webhook told of every one.
    pub fn page(&self) -> ito_prefs::Notify {
        ito_prefs::Notify {
            system: vec![self.system],
            sound: vec![self.sound],
            webhook: Vec::new(),
            sounds: vec![self.sound_name.clone()],
            focus: self.focus_mode,
            webhook_url: self.webhook.clone(),
            webhook_format: self.webhook_format.clone(),
        }
    }

    /// The `[notify]` keys to write so that this reads as the page's `p`.
    pub fn edits(&self, p: &ito_prefs::Notify) -> Vec<(&'static str, String)> {
        use ito_common::quote;
        let first = |v: &[bool], was: bool| v.first().copied().unwrap_or(was);
        let mut out = Vec::new();
        for (key, was, now) in [
            ("system", self.system, first(&p.system, self.system)),
            ("sound", self.sound, first(&p.sound, self.sound)),
            ("focus_mode", self.focus_mode, p.focus),
        ] {
            if was != now {
                out.push((key, now.to_string()));
            }
        }
        let name = p.sounds.first().unwrap_or(&self.sound_name);
        for (key, was, now) in [
            ("sound_name", &self.sound_name, name),
            ("webhook", &self.webhook, &p.webhook_url),
            ("webhook_format", &self.webhook_format, &p.webhook_format),
        ] {
            if was != now {
                out.push((key, quote(now)));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_writes_only_what_changed() {
        let was = Notify::default();
        let mut p = was.page();
        assert!(was.edits(&p).is_empty());
        p.sound[0] = true;
        p.sounds[0] = "chime".into();
        p.webhook_url = "http://127.0.0.1:8765/hook".into();
        assert_eq!(
            was.edits(&p),
            [
                ("sound", "true".to_owned()),
                ("sound_name", "\"chime\"".to_owned()),
                ("webhook", "\"http://127.0.0.1:8765/hook\"".to_owned())
            ]
        );
    }

    fn rules(text: &str) -> (Vec<Rule>, Vec<String>) {
        let t: toml::Table = text.parse().unwrap();
        let mut warn = Vec::new();
        (parse_rules(&t["alert"], &mut warn), warn)
    }

    #[test]
    fn rules_read_and_the_bad_ones_are_warned_about() {
        let (r, w) =
            rules("[[alert]]\nmetric = \"cpu\"\nabove = 90\nfor = 30\nname = \"busy\"\n[[alert]]\nmetric = \"battery\"\nbelow = 15.5\n");
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(r[0], Rule { metric: Metric::Cpu, limit: 90.0, above: true, secs: 30, name: Some("busy".into()), service: None });
        assert_eq!((r[1].metric, r[1].above, r[1].secs), (Metric::Battery, false, 10), "for defaults to 10");
        let (r, w) = rules(
            "[[alert]]\nmetric = \"fans\"\nabove = 1\n[[alert]]\nmetric = \"cpu\"\n[[alert]]\nmetric = \"cpu\"\nabove = 1\nbelow = 2\n[[alert]]\nmetric = \"cpu\"\nabove = 1\nfor = -3\nx = 1\n",
        );
        assert!(r.is_empty());
        assert!(w.len() >= 5, "{w:?}");
    }

    #[test]
    fn notify_reads_its_words() {
        let t: toml::Table =
            "[notify]\nsound = true\nsound_name = \"low\"\nwebhook = \"https://ntfy.sh/x\"\nwebhook_format = \"slack\"\n".parse().unwrap();
        let mut w = Vec::new();
        let n = parse_notify(&t["notify"], &mut w);
        assert!(w.is_empty() && n.sound && n.system && n.sound_name == "low" && n.webhook_format == "slack");
        let t: toml::Table = "[notify]\nwebhook_format = \"discord\"\n".parse().unwrap();
        let mut w = Vec::new();
        assert_eq!(parse_notify(&t["notify"], &mut w).webhook_format, "discord");
        assert!(w.is_empty(), "{w:?}");
        let t: toml::Table =
            "[notify]\nwebhook = \"file:///x\"\nsound_name = \"boom\"\nwebhook_format = \"xml\"\nsystem = 3\nmore = 1\n".parse().unwrap();
        let mut w = Vec::new();
        // The address that is not http stays, for the settings to show in red.
        assert_eq!(parse_notify(&t["notify"], &mut w), Notify { webhook: "file:///x".into(), ..Notify::default() });
        assert_eq!(w.len(), 5, "{w:?}");
    }

    #[test]
    fn a_rule_fires_once_after_it_has_held_and_again_after_it_clears() {
        let rule = Rule { metric: Metric::Cpu, limit: 90.0, above: true, secs: 30, name: None, service: None };
        let mut t = Tracker::default();
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        let list = [rule];
        assert!(t.step(&list, at(0), |_| Some(95.0)).is_empty(), "it has not held yet");
        assert!(t.step(&list, at(29), |_| Some(99.0)).is_empty());
        let f = t.step(&list, at(30), |_| Some(97.0));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].value, 97.0);
        assert!(t.step(&list, at(60), |_| Some(97.0)).is_empty(), "once");
        // A dip below the limit starts it over.
        assert!(t.step(&list, at(61), |_| Some(10.0)).is_empty());
        assert!(t.step(&list, at(62), |_| Some(95.0)).is_empty());
        assert_eq!(t.step(&list, at(92), |_| Some(95.0)).len(), 1);
        // No reading is not a breach.
        assert!(t.step(&list, at(200), |_| None).is_empty());
    }

    #[test]
    fn under_and_changed_rules() {
        let low = Rule { metric: Metric::Battery, limit: 15.0, above: false, secs: 0, name: None, service: None };
        let mut t = Tracker::default();
        let now = Instant::now();
        assert_eq!(t.step(std::slice::from_ref(&low), now, |_| Some(10.0)).len(), 1, "no wait with for = 0");
        assert!(t.step(std::slice::from_ref(&low), now, |_| Some(10.0)).is_empty());
        // Another list: it says so again.
        let mut other = low;
        other.limit = 20.0;
        assert_eq!(t.step(&[other], now, |_| Some(10.0)).len(), 1);
    }

    #[test]
    fn a_service_rule_wants_a_name_and_no_number() {
        let (r, w) = rules("[[alert]]\nmetric = \"service\"\nservice = \"nginx\"\nfor = 5\n");
        assert!(w.is_empty(), "{w:?}");
        assert_eq!((r[0].metric, r[0].service.as_deref(), r[0].secs), (Metric::Service, Some("nginx"), 5));
        let (r, w) = rules(
            "[[alert]]\nmetric = \"service\"\n[[alert]]\nmetric = \"service\"\nservice = \"a\"\nabove = 1\n[[alert]]\nmetric = \"cpu\"\nabove = 1\nservice = \"a\"\n",
        );
        assert!(r.is_empty());
        assert_eq!(w.len(), 3, "{w:?}");
    }

    #[test]
    fn a_service_reads_one_while_it_is_down() {
        let rule = Rule { metric: Metric::Service, limit: 0.5, above: true, secs: 0, name: None, service: Some("NGINX".into()) };
        let mut m = Model::default();
        assert_eq!(rule.read(&m), None, "not listed");
        let mut s = svc::Service { name: "nginx".into(), state: svc::State::Running, ..Default::default() };
        m.services.list.push(s.clone());
        assert_eq!(rule.read(&m), Some(0.0));
        for (state, want) in [(svc::State::Stopped, Some(1.0)), (svc::State::Failed, Some(1.0)), (svc::State::Stopping, None)] {
            s.state = state;
            m.services.list[0] = s.clone();
            assert_eq!(rule.read(&m), want, "{state:?}");
        }
        let fired = Fired { rule, value: 1.0 };
        assert!(!fired.words("").0.is_empty());
    }

    #[test]
    fn readings_come_from_the_model() {
        let mut m = Model::default();
        assert_eq!(Metric::Cpu.read(&m), None, "nothing sampled yet");
        m.mem.total = 200;
        m.mem.used = 50;
        assert_eq!(Metric::Mem.read(&m), Some(25.0));
        assert_eq!(Metric::Swap.read(&m), None, "no swap");
        assert_eq!(Metric::Temp.read(&m), None);
    }
}
