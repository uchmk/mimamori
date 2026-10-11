//! The settings files, all under one folder shared by uchmk's apps:
//!
//! ```text
//! <config dir>/uchmk/common.toml          language = "auto" | "en" | …
//! <config dir>/uchmk/yagura/config.toml  interval, language, backend,
//!                                         [agent], [[host]]
//! <config dir>/uchmk/yagura/keymap.toml  laid over the built-in keymap
//! ```
//!
//! `<config dir>` is the OS's (`%APPDATA%`, `~/Library/Application Support`,
//! `~/.config`); `UCHMK_CONFIG_DIR` points the whole `uchmk` folder elsewhere
//! and `YAGURA_CONFIG_HOME` yagura's own. The column widths yagura writes
//! for itself go to the state folder (`crate::persist`).
//! Before v0.33.0 the app was called mimamori and its folder `mimamori/`;
//! [`load`] moves that folder to `yagura/` the first time it finds it alone
//! (or copies it, when another program keeps it from being renamed).

use std::path::{Path, PathBuf};

use crate::remote::noise::{self, Key};
use crate::remote::proto::{PACE_MAX, PACE_MIN};

pub const INTERVAL_MIN: f32 = 0.5;
pub const INTERVAL_MAX: f32 = 10.0;
pub const BACKENDS: [&str; 5] = ["auto", "gl", "vulkan", "dx12", "metal"];
/// What `[agent] allow` may let a fleet viewer do on this machine.
pub const ALLOW: [&str; 2] = ["kill", "service"];
/// yagura's own folder in the `uchmk` one.
pub const DIR: &str = "yagura";
/// The folder's name before v0.33.0, when the app was called mimamori.
const OLD_DIR: &str = "mimamori";
/// Puts yagura's own folder anywhere, in place of `<uchmk>/yagura`.
const CONFIG_HOME: &str = "YAGURA_CONFIG_HOME";

/// yagura's own folder for the `uchmk` folder `base`: `YAGURA_CONFIG_HOME`
/// when set, else `base/yagura`.
pub fn own_dir(base: &Path) -> PathBuf {
    ito_common::env_dir(CONFIG_HOME).unwrap_or_else(|| base.join(DIR))
}

/// A machine on the fleet screen: `[[host]] name = "nas", addr = "localhost:7314"`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Host {
    pub name: String,
    /// `host` or `host:port`; the port defaults to the agent's (the direct
    /// one with a `key`).
    pub addr: String,
    /// The agent's public key: connect to its `[agent] listen` directly,
    /// over Noise, rather than to a tunnel.
    pub key: Option<Key>,
    /// Seconds between its summaries, for a thin line; the agent's interval
    /// when `None`.
    pub every: Option<f32>,
}

/// A viewer let in directly: `[[agent.viewer]] name = "laptop", key = "…"`.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewerKey {
    pub name: String,
    pub key: Key,
}

#[derive(Clone, Debug)]
pub struct Settings {
    /// Seconds between samples.
    pub interval: f32,
    /// The language picked from the app's, the common and the OS's setting.
    pub language: String,
    /// config.toml's own `language`, which beats common.toml's unless it is
    /// `auto`.
    pub app_language: Option<String>,
    /// The graphics backend asked for; `None` is `auto`.
    pub backend: Option<String>,
    /// The user's keymap.toml, if there is one.
    pub keymap: Option<String>,
    /// yagura's own folder (where state.toml was before v0.38.0); `None`
    /// when the OS has no config directory.
    pub dir: Option<PathBuf>,
    /// `[agent] allow`: what a fleet viewer may do here (nothing by default).
    pub allow: Vec<String>,
    /// `[agent] port`: where `--agent` listens on 127.0.0.1.
    pub port: u16,
    /// `[agent] listen`: where `--agent` also listens for viewers with a key
    /// (`address:port`).
    pub listen: Option<String>,
    /// `[[agent.viewer]]`: the keys let in there.
    pub viewers: Vec<ViewerKey>,
    /// `[[host]]`: the other machines on the fleet screen.
    pub hosts: Vec<Host>,
    /// `[notify]`: how a fired alert reaches a person.
    pub notify: crate::alert::Notify,
    /// `[[alert]]`: the threshold rules.
    pub alerts: Vec<crate::alert::Rule>,
    pub warnings: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            interval: 1.0,
            language: "en".into(),
            app_language: None,
            backend: None,
            keymap: None,
            dir: None,
            allow: Vec::new(),
            port: crate::remote::proto::PORT,
            listen: None,
            viewers: Vec::new(),
            hosts: Vec::new(),
            notify: crate::alert::Notify::default(),
            alerts: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

/// The `uchmk` folder (the same for every uchmk app; `ito-i18n`).
pub fn base_dir() -> Option<PathBuf> {
    ito_i18n::base_dir()
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

/// A key in base64, or why not.
fn key(t: &toml::Table, file: &str, warn: &mut Vec<String>) -> Result<Option<Key>, ()> {
    let Some(text) = string(t, "key", file, warn) else { return Ok(None) };
    match noise::decode(&text) {
        Some(k) => Ok(Some(k)),
        None => {
            warn.push(format!("{file}: `key` is not a key (44 characters of base64, as `yagura --key` prints it)"));
            Err(())
        }
    }
}

/// `[[agent.viewer]]`: each with a `name` and a `key`.
fn viewers(v: &toml::Value, warn: &mut Vec<String>) -> Vec<ViewerKey> {
    let Some(list) = v.as_array() else {
        warn.push("config.toml: `agent.viewer` must be [[agent.viewer]] tables".into());
        return Vec::new();
    };
    let mut out = Vec::new();
    for (i, x) in list.iter().enumerate() {
        let file = format!("config.toml [[agent.viewer]] {}", i + 1);
        let Some(t) = x.as_table() else {
            warn.push(format!("{file} must be a table"));
            continue;
        };
        for k in t.keys().filter(|k| !["key", "name"].contains(&k.as_str())) {
            warn.push(format!("{file}: unknown setting `{k}`"));
        }
        let name = string(t, "name", &file, warn).filter(|n| !n.trim().is_empty());
        match key(t, &file, warn) {
            Ok(Some(key)) => out.push(ViewerKey { name: name.unwrap_or_else(|| noise::encode(&key)[..8].to_string()), key }),
            Ok(None) => warn.push(format!("{file}: needs a `key`")),
            Err(()) => {}
        }
    }
    out
}

/// `[agent]`: `allow = ["kill", "service"]`, `port = 7313`, `listen =
/// "0.0.0.0:7314"` and `[[agent.viewer]]`.
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
            "listen" => match v.as_str().map(str::trim) {
                Some(a) if !a.is_empty() => s.listen = Some(crate::remote::proto::with_port(a, crate::remote::proto::DIRECT_PORT)),
                _ => warn.push("config.toml: [agent] `listen` is an address such as \"0.0.0.0:7314\"".into()),
            },
            "viewer" => s.viewers = viewers(v, warn),
            other => warn.push(format!("config.toml: unknown setting `agent.{other}`")),
        }
    }
}

/// `[[host]]`: each with an `addr`, and a `name`, `key` and `every` if it
/// likes.
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
        for k in t.keys().filter(|k| !["addr", "every", "key", "name"].contains(&k.as_str())) {
            warn.push(format!("{file}: unknown setting `{k}`"));
        }
        // A host with a bad key is left out rather than reached some other way.
        let Ok(key) = key(t, &file, warn) else { continue };
        let every = match t.get("every").map(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64))) {
            None => None,
            Some(Some(f)) if (PACE_MIN as f64..=PACE_MAX as f64).contains(&f) => Some(f as f32),
            Some(_) => {
                warn.push(format!("{file}: `every` is seconds from {PACE_MIN} to {PACE_MAX}"));
                None
            }
        };
        match addr.filter(|a| !a.trim().is_empty()) {
            Some(addr) => {
                let addr = addr.trim().to_string();
                let name = name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| addr.clone());
                out.push(Host { name, addr, key, every });
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
    let mut s = Settings::default();
    let mut common_lang = None;
    let mut app_lang = None;
    if let Some(base) = &base {
        match ito_i18n::read_common_language(base) {
            Ok(l) => common_lang = l,
            Err(e) => warn.push(e),
        }
        let (dir, note) = match ito_common::env_dir(CONFIG_HOME) {
            Some(d) => (d, None),
            None => move_old_dir(base),
        };
        warn.extend(note);
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
                    "notify" => s.notify = crate::alert::parse_notify(v, &mut warn),
                    "alert" => s.alerts = crate::alert::parse_rules(v, &mut warn),
                    other => warn.push(format!("config.toml: unknown setting `{other}`")),
                }
            }
        }
        s.keymap = std::fs::read_to_string(dir.join("keymap.toml")).ok();
        s.dir = Some(dir);
    }
    let os = ito_i18n::os_language();
    s.language = crate::i18n::resolve_language(app_lang.as_deref(), common_lang.as_deref(), os.as_deref());
    s.app_language = app_lang;
    s.warnings = warn;
    s
}

/// Move `mimamori/` to `yagura/` when only the old one is there, so the
/// settings, keymap and column widths written before the rename carry over;
/// the folder to read this time and what to tell. With both there, the new
/// one is used and the old one left alone.
fn move_old_dir(base: &Path) -> (PathBuf, Option<String>) {
    carry_over(&base.join(OLD_DIR), &base.join(DIR), |a, b| std::fs::rename(a, b))
}

/// `old` renamed to `new`, else copied there and left (Windows will not
/// rename a folder while a file in it is open, or Explorer or a shell is in
/// it), else read where it is this time and tried again next time.
fn carry_over(old: &Path, new: &Path, rename: impl Fn(&Path, &Path) -> std::io::Result<()>) -> (PathBuf, Option<String>) {
    if !old.is_dir() || new.exists() {
        return (new.to_path_buf(), None);
    }
    let Err(e) = rename(old, new) else { return (new.to_path_buf(), None) };
    match copy_dir(old, new) {
        Ok(()) => {
            let note = format!(
                "could not move {} ({e}), so it was copied to {}; delete the old folder once nothing uses it",
                old.display(),
                new.display()
            );
            (new.to_path_buf(), Some(note))
        }
        Err(c) => {
            // `new` was not there before: take the half copy away so the next
            // start tries again.
            let _ = std::fs::remove_dir_all(new);
            (old.to_path_buf(), Some(format!("could not move or copy {} to {} ({e}; {c}): read where it is", old.display(), new.display())))
        }
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
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
        assert_eq!(s.dir, Some(d.join("yagura")));
    }

    #[test]
    fn the_folder_from_before_the_rename_is_moved() {
        let d = test_dir("settings-old-dir");
        std::fs::create_dir_all(d.join("mimamori")).unwrap();
        std::fs::write(d.join("mimamori/config.toml"), "interval = 2\n").unwrap();
        let s = load(Some(&d));
        assert_eq!(s.interval, 2.0, "{:?}", s.warnings);
        assert!(!d.join("mimamori").exists() && d.join("yagura/config.toml").is_file());

        // With both there, the new one wins and the old one stays as it is.
        std::fs::create_dir_all(d.join("mimamori")).unwrap();
        std::fs::write(d.join("mimamori/config.toml"), "interval = 3\n").unwrap();
        let s = load(Some(&d));
        assert_eq!(s.interval, 2.0);
        assert!(d.join("mimamori/config.toml").is_file() && s.warnings.is_empty(), "{:?}", s.warnings);
    }

    #[test]
    fn a_folder_in_use_is_copied_instead() {
        let d = test_dir("settings-old-in-use");
        let (old, new) = (d.join("mimamori"), d.join("yagura"));
        std::fs::create_dir_all(old.join("themes")).unwrap();
        std::fs::write(old.join("config.toml"), "interval = 2\n").unwrap();
        std::fs::write(old.join("themes/mine.toml"), "bg = \"#000000\"\n").unwrap();
        // What Windows says while Explorer shows the folder (os error 32).
        let busy = |_: &Path, _: &Path| Err(std::io::Error::other("in use"));
        let (dir, note) = carry_over(&old, &new, busy);
        assert_eq!(dir, new);
        assert!(note.is_some_and(|n| n.contains("copied")));
        assert!(new.join("config.toml").is_file() && new.join("themes/mine.toml").is_file());
        assert!(old.join("config.toml").is_file(), "the old one stays");
        // Next time the new one is there and nothing more is said.
        assert_eq!(carry_over(&old, &new, busy), (new.clone(), None));
        assert_eq!(load(Some(&d)).interval, 2.0);
    }

    #[test]
    fn reads_the_files_and_warns_about_bad_values() {
        let d = test_dir("settings-read");
        std::fs::create_dir_all(d.join("yagura")).unwrap();
        std::fs::write(d.join("common.toml"), "language = \"ja\"\n").unwrap();
        std::fs::write(d.join("yagura/config.toml"), "interval = 2\nbackend = \"gl\"\nlanguage = \"auto\"\ncolour = 1\n").unwrap();
        std::fs::write(d.join("yagura/keymap.toml"), "[main]\n").unwrap();
        let s = load(Some(&d));
        assert_eq!(s.interval, 2.0);
        assert_eq!(s.backend.as_deref(), Some("gl"));
        assert_eq!(s.keymap.as_deref(), Some("[main]\n"));
        // The app's "auto" passes the choice on to the common file.
        assert_eq!(s.language, "ja");
        assert_eq!(s.warnings.len(), 1, "{:?}", s.warnings);
        assert!(s.warnings[0].contains("colour"));

        std::fs::write(d.join("yagura/config.toml"), "interval = 60\nbackend = \"glide\"\n").unwrap();
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
        std::fs::create_dir_all(d.join("yagura")).unwrap();
        let text = "[agent]\nallow = [\"kill\", \"reboot\", \"kill\"]\nport = 7400\n\n\
                    [[host]]\nname = \"nas\"\naddr = \"localhost:7314\"\n\n\
                    [[host]]\naddr = \"pi.tail1234.ts.net\"\n\n\
                    [[host]]\nname = \"broken\"\ncolour = 1\n";
        std::fs::write(d.join("yagura/config.toml"), text).unwrap();
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
        std::fs::write(d.join("yagura/config.toml"), "").unwrap();
        let s = load(Some(&d));
        assert!(s.allow.is_empty() && s.hosts.is_empty() && s.warnings.is_empty(), "{s:?}");
        assert_eq!(s.port, crate::remote::proto::PORT);
        assert!(s.listen.is_none() && s.viewers.is_empty());
    }

    #[test]
    fn reads_the_keys_for_direct_connections() {
        let d = test_dir("settings-direct");
        std::fs::create_dir_all(d.join("yagura")).unwrap();
        let (a, b) = (noise::encode(&[1; 32]), noise::encode(&[2; 32]));
        let text = format!(
            "[agent]\nlisten = \"0.0.0.0\"\n\n\
             [[agent.viewer]]\nname = \"laptop\"\nkey = \"{a}\"\n\n\
             [[agent.viewer]]\nkey = \"{b}\"\n\n\
             [[agent.viewer]]\nname = \"bad\"\nkey = \"short\"\n\n\
             [[host]]\nname = \"nas\"\naddr = \"nas.lan\"\nkey = \"{a}\"\nevery = 10\n\n\
             [[host]]\nname = \"bad key\"\naddr = \"x\"\nkey = \"short\"\n\n\
             [[host]]\naddr = \"slow\"\nevery = 600\n"
        );
        std::fs::write(d.join("yagura/config.toml"), text).unwrap();
        let s = load(Some(&d));
        assert_eq!(s.listen.as_deref(), Some("0.0.0.0:7314"));
        assert_eq!(s.viewers, [ViewerKey { name: "laptop".into(), key: [1; 32] }, ViewerKey { name: b[..8].to_string(), key: [2; 32] }]);
        // A bad key leaves its host out; a bad `every` only the pace.
        assert_eq!(
            s.hosts,
            [
                Host { name: "nas".into(), addr: "nas.lan".into(), key: Some([1; 32]), every: Some(10.0) },
                Host { name: "slow".into(), addr: "slow".into(), ..Host::default() },
            ]
        );
        let w = s.warnings.join("\n");
        assert_eq!(s.warnings.len(), 3, "{w}");
        assert!(w.contains("[[agent.viewer]] 3") && w.contains("[[host]] 2") && w.contains("`every`"), "{w}");
    }
}
