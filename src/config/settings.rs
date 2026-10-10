//! The settings files, all under one folder shared by uchmk's apps:
//!
//! ```text
//! <config dir>/uchmk/common.toml          language = "auto" | "en" | …
//! <config dir>/uchmk/mimamori/config.toml interval, language, backend,
//!                                         [agent], [[host]]
//! <config dir>/uchmk/mimamori/keymap.toml laid over the built-in keymap
//! <config dir>/uchmk/mimamori/state.toml  the column widths (written by mimamori)
//! ```
//!
//! `<config dir>` is the OS's (`%APPDATA%`, `~/Library/Application Support`,
//! `~/.config`); `UCHMK_CONFIG_DIR` points the whole `uchmk` folder elsewhere.

use std::path::{Path, PathBuf};

pub const INTERVAL_MIN: f32 = 0.5;
pub const INTERVAL_MAX: f32 = 10.0;
pub const BACKENDS: [&str; 5] = ["auto", "gl", "vulkan", "dx12", "metal"];
/// What `[agent] allow` may let a fleet viewer do on this machine.
pub const ALLOW: [&str; 2] = ["kill", "service"];

/// A machine on the fleet screen: `[[host]] name = "nas", addr = "localhost:7314"`.
#[derive(Clone, Debug, PartialEq)]
pub struct Host {
    pub name: String,
    /// `host` or `host:port`; the port defaults to the agent's.
    pub addr: String,
}

#[derive(Debug)]
pub struct Settings {
    /// Seconds between samples.
    pub interval: f32,
    /// The language picked from the app's, the common and the OS's setting.
    pub language: String,
    /// The graphics backend asked for; `None` is `auto`.
    pub backend: Option<String>,
    /// The user's keymap.toml, if there is one.
    pub keymap: Option<String>,
    /// mimamori's own folder, where state.toml goes; `None` when the OS has
    /// no config directory.
    pub dir: Option<PathBuf>,
    /// `[agent] allow`: what a fleet viewer may do here (nothing by default).
    pub allow: Vec<String>,
    /// `[agent] port`: where `--agent` listens on 127.0.0.1.
    pub port: u16,
    /// `[[host]]`: the other machines on the fleet screen.
    pub hosts: Vec<Host>,
    pub warnings: Vec<String>,
}

/// The `uchmk` folder (the same for every uchmk app; `tsumugi-i18n`).
pub fn base_dir() -> Option<PathBuf> {
    tsumugi_i18n::base_dir()
}

fn read_table(path: &Path, warn: &mut Vec<String>) -> Option<toml::Table> {
    let text = std::fs::read_to_string(path).ok()?;
    match text.parse::<toml::Table>() {
        Ok(t) => Some(t),
        Err(e) => {
            warn.push(format!("{}: {e}", path.display()));
            None
        }
    }
}

fn string(t: &toml::Table, key: &str, file: &str, warn: &mut Vec<String>) -> Option<String> {
    match t.get(key)? {
        toml::Value::String(s) => Some(s.clone()),
        _ => {
            warn.push(format!("{file}: `{key}` must be a string"));
            None
        }
    }
}

/// `[agent]`: `allow = ["kill", "service"]`, `port = 7313`.
fn agent(v: &toml::Value, s: &mut Settings, warn: &mut Vec<String>) {
    let Some(t) = v.as_table() else {
        warn.push("config.toml: `agent` must be a table".into());
        return;
    };
    for (k, v) in t {
        match k.as_str() {
            "allow" => match v.as_array() {
                Some(list) => {
                    for x in list {
                        match x.as_str() {
                            Some(a) if ALLOW.contains(&a) => {
                                if !s.allow.iter().any(|b| b == a) {
                                    s.allow.push(a.to_string());
                                }
                            }
                            _ => warn.push(format!("config.toml: [agent] allow takes {}, not {x}", ALLOW.join(", "))),
                        }
                    }
                }
                None => warn.push("config.toml: [agent] `allow` must be a list".into()),
            },
            "port" => match v.as_integer() {
                Some(p) if (1..=65535).contains(&p) => s.port = p as u16,
                _ => warn.push("config.toml: [agent] `port` is a number from 1 to 65535".into()),
            },
            other => warn.push(format!("config.toml: unknown setting `agent.{other}`")),
        }
    }
}

/// `[[host]]`: each with a `name` and an `addr`.
fn hosts(v: &toml::Value, warn: &mut Vec<String>) -> Vec<Host> {
    let Some(list) = v.as_array() else {
        warn.push("config.toml: `host` must be [[host]] tables".into());
        return Vec::new();
    };
    let mut out = Vec::new();
    for (i, h) in list.iter().enumerate() {
        let n = i + 1;
        let Some(t) = h.as_table() else {
            warn.push(format!("config.toml: [[host]] {n} must be a table"));
            continue;
        };
        let file = format!("config.toml [[host]] {n}");
        let addr = string(t, "addr", &file, warn);
        let name = string(t, "name", &file, warn);
        for k in t.keys().filter(|k| !["addr", "name"].contains(&k.as_str())) {
            warn.push(format!("{file}: unknown setting `{k}`"));
        }
        match addr.filter(|a| !a.trim().is_empty()) {
            Some(addr) => {
                let addr = addr.trim().to_string();
                out.push(Host { name: name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| addr.clone()), addr });
            }
            None => warn.push(format!("{file}: needs an `addr`")),
        }
    }
    out
}

/// Read everything from `base` (the `uchmk` folder; [`base_dir`] when
/// `None`). A missing file is the defaults; a bad value is warned about and
/// left at its default.
pub fn load(base: Option<&Path>) -> Settings {
    let base = base.map(Path::to_path_buf).or_else(base_dir);
    let mut warn = Vec::new();
    let mut s = Settings {
        interval: 1.0,
        language: "en".into(),
        backend: None,
        keymap: None,
        dir: None,
        allow: Vec::new(),
        port: crate::remote::proto::PORT,
        hosts: Vec::new(),
        warnings: Vec::new(),
    };
    let mut common_lang = None;
    let mut app_lang = None;
    if let Some(base) = &base {
        match tsumugi_i18n::read_common_language(base) {
            Ok(l) => common_lang = l,
            Err(e) => warn.push(e),
        }
        let dir = base.join("mimamori");
        if let Some(t) = read_table(&dir.join("config.toml"), &mut warn) {
            for (k, v) in &t {
                match k.as_str() {
                    "interval" => match v.as_float().or_else(|| v.as_integer().map(|i| i as f64)) {
                        Some(f) if (INTERVAL_MIN as f64..=INTERVAL_MAX as f64).contains(&f) => s.interval = f as f32,
                        _ => warn.push(format!("config.toml: `interval` is seconds from {INTERVAL_MIN} to {INTERVAL_MAX}")),
                    },
                    "language" => app_lang = string(&t, "language", "config.toml", &mut warn),
                    "backend" => match string(&t, "backend", "config.toml", &mut warn) {
                        Some(b) if BACKENDS.contains(&b.as_str()) => s.backend = (b != "auto").then_some(b),
                        Some(b) => warn.push(format!("config.toml: unknown backend `{b}` (one of {})", BACKENDS.join(", "))),
                        None => {}
                    },
                    "agent" => agent(v, &mut s, &mut warn),
                    "host" => s.hosts = hosts(v, &mut warn),
                    other => warn.push(format!("config.toml: unknown setting `{other}`")),
                }
            }
        }
        s.keymap = std::fs::read_to_string(dir.join("keymap.toml")).ok();
        s.dir = Some(dir);
    }
    let os = tsumugi_i18n::os_language();
    s.language = crate::i18n::resolve_language(app_lang.as_deref(), common_lang.as_deref(), os.as_deref());
    s.warnings = warn;
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_dir;

    #[test]
    fn nothing_on_disk_is_the_defaults() {
        let d = test_dir("settings-empty");
        let s = load(Some(&d));
        assert_eq!(s.interval, 1.0);
        assert!(s.backend.is_none() && s.keymap.is_none() && s.warnings.is_empty(), "{s:?}");
        assert_eq!(s.dir, Some(d.join("mimamori")));
    }

    #[test]
    fn reads_the_files_and_warns_about_bad_values() {
        let d = test_dir("settings-read");
        std::fs::create_dir_all(d.join("mimamori")).unwrap();
        std::fs::write(d.join("common.toml"), "language = \"ja\"\n").unwrap();
        std::fs::write(d.join("mimamori/config.toml"), "interval = 2\nbackend = \"gl\"\nlanguage = \"auto\"\ncolour = 1\n").unwrap();
        std::fs::write(d.join("mimamori/keymap.toml"), "[main]\n").unwrap();
        let s = load(Some(&d));
        assert_eq!(s.interval, 2.0);
        assert_eq!(s.backend.as_deref(), Some("gl"));
        assert_eq!(s.keymap.as_deref(), Some("[main]\n"));
        // The app's "auto" passes the choice on to the common file.
        assert_eq!(s.language, "ja");
        assert_eq!(s.warnings.len(), 1, "{:?}", s.warnings);
        assert!(s.warnings[0].contains("colour"));

        std::fs::write(d.join("mimamori/config.toml"), "interval = 60\nbackend = \"glide\"\n").unwrap();
        let s = load(Some(&d));
        assert_eq!(s.interval, 1.0);
        assert!(s.backend.is_none());
        assert_eq!(s.warnings.len(), 2, "{:?}", s.warnings);

        // A bad common.toml is said, and the language falls to the OS's.
        std::fs::write(d.join("common.toml"), "language = 1\n").unwrap();
        let s = load(Some(&d));
        assert!(s.warnings.iter().any(|w| w.contains("common.toml")), "{:?}", s.warnings);
    }

    #[test]
    fn reads_the_agent_and_the_hosts() {
        let d = test_dir("settings-fleet");
        std::fs::create_dir_all(d.join("mimamori")).unwrap();
        let text = "[agent]\nallow = [\"kill\", \"reboot\", \"kill\"]\nport = 7400\n\n\
                    [[host]]\nname = \"nas\"\naddr = \"localhost:7314\"\n\n\
                    [[host]]\naddr = \"pi.tail1234.ts.net\"\n\n\
                    [[host]]\nname = \"broken\"\ncolour = 1\n";
        std::fs::write(d.join("mimamori/config.toml"), text).unwrap();
        let s = load(Some(&d));
        assert_eq!(s.allow, ["kill"]);
        assert_eq!(s.port, 7400);
        let hosts: Vec<(&str, &str)> = s.hosts.iter().map(|h| (h.name.as_str(), h.addr.as_str())).collect();
        // A host without a name goes by its address; one without an address is left out.
        assert_eq!(hosts, [("nas", "localhost:7314"), ("pi.tail1234.ts.net", "pi.tail1234.ts.net")]);
        let w = s.warnings.join("\n");
        assert!(w.contains("reboot") && w.contains("colour") && w.contains("needs an `addr`"), "{w}");
        assert_eq!(s.warnings.len(), 3, "{w}");

        // Nothing said is view only, on the default port, with no other hosts.
        std::fs::write(d.join("mimamori/config.toml"), "").unwrap();
        let s = load(Some(&d));
        assert!(s.allow.is_empty() && s.hosts.is_empty() && s.warnings.is_empty(), "{s:?}");
        assert_eq!(s.port, crate::remote::proto::PORT);
    }
}
