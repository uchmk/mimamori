//! `yagura env`: everything a bug report would otherwise have to ask for.
//!
//! Modelled on kura's `kura env` (and yazi's `ya env`): which config files
//! were read, what the settings came out as, which helper tools are on the
//! PATH, which variables are set. It prints rather than opens a browser,
//! because this is text to paste into a report that already exists.

use crate::config::settings::{self, Settings};
use std::path::{Path, PathBuf};

type Rows = Vec<(String, String)>;

/// The whole report.
pub fn text() -> String {
    let s = settings::load(None);
    let mut out = String::new();
    section(&mut out, "Yagura", &version());
    section(&mut out, "Files", &files(&s));
    section(&mut out, "Settings", &settings_rows(&s));
    section(&mut out, "Tools", &tools());
    section(&mut out, "Variables", &variables());
    // One newline after the last row, not the blank line every section ends with.
    out.truncate(out.trim_end_matches('\n').len());
    out.push('\n');
    out
}

fn row(k: &str, v: impl Into<String>) -> (String, String) {
    (k.to_string(), v.into())
}

fn section(out: &mut String, title: &str, rows: &[(String, String)]) {
    out.push_str(title);
    out.push('\n');
    // A key longer than this goes on a line of its own with the value under
    // it, so one long row does not push the others past a console's width.
    const MAX_KEY: usize = 24;
    let width = rows.iter().map(|(k, _)| k.chars().count()).filter(|&n| n <= MAX_KEY).max().unwrap_or(0);
    for (k, v) in rows {
        let mut lines = v.split('\n');
        let first = lines.next().unwrap_or_default();
        if k.chars().count() > MAX_KEY {
            out.push_str(&format!("    {k}\n    {:width$}   {first}\n", ""));
        } else {
            out.push_str(&format!("    {k:width$} : {first}\n"));
        }
        for rest in lines {
            out.push_str(&format!("    {:width$}   {rest}\n", ""));
        }
    }
    out.push('\n');
}

fn version() -> Rows {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|e| format!("unknown ({e})"));
    vec![
        row("Version", env!("CARGO_PKG_VERSION")),
        row("OS", format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)),
        row("Executable", exe),
    ]
}

fn files(s: &Settings) -> Rows {
    let base = settings::base_dir();
    let own = s.dir.clone();
    let shown = |p: Option<PathBuf>| p.map(|p| p.display().to_string()).unwrap_or_else(|| "none (this OS has no config folder)".into());
    let mut rows = vec![row("uchmk folder", shown(base.clone())), row("yagura folder", shown(own.clone()))];
    if let Some(base) = &base {
        rows.push(row("common.toml", exists(&base.join("common.toml"))));
        rows.push(row("themes", exists(&base.join("themes"))));
    }
    if let Some(own) = &own {
        for name in ["config.toml", "keymap.toml", "key"] {
            rows.push(row(name, exists(&own.join(name))));
        }
    }
    let (state, _) = crate::persist::dir(own.as_deref());
    rows.push(row("state folder", shown(state.clone())));
    if let Some(state) = &state {
        rows.push(row("state.toml", exists(&state.join("state.toml"))));
    }
    rows
}

fn exists(p: &Path) -> String {
    match std::fs::metadata(p) {
        Ok(m) if m.is_dir() => "found".into(),
        Ok(m) => format!("found ({} bytes)", m.len()),
        Err(_) => "not found".into(),
    }
}

fn settings_rows(s: &Settings) -> Rows {
    let on = |b: bool| if b { "on" } else { "off" };
    let mut rows = vec![
        row("interval", format!("{} s", s.interval)),
        row("language", s.language.clone()),
        row("backend", s.backend.clone().unwrap_or_else(|| "auto".into())),
        row("check_updates", on(s.check_updates)),
        row("agent port", s.port.to_string()),
        row("agent listen", s.listen.clone().unwrap_or_else(|| "off".into())),
        row("agent metrics", s.metrics.clone().unwrap_or_else(|| "off".into())),
        row("agent allow", if s.allow.is_empty() { "nothing".into() } else { s.allow.join(", ") }),
        row("agent viewers", s.viewers.len().to_string()),
        row("hosts", s.hosts.len().to_string()),
        row("alerts", s.alerts.len().to_string()),
        row("keymap.toml", if s.keymap.is_some() { "read" } else { "none" }),
    ];
    // What the loader complained about is the first thing a report needs.
    for (i, w) in s.warnings.iter().enumerate() {
        rows.push(row(&format!("warning {}", i + 1), w.clone()));
    }
    rows
}

fn tools() -> Rows {
    let names: &[(&str, &str)] = &[
        ("curl", "the update check"),
        ("smartctl", "drive health on Linux"),
        ("systemctl", "services on Linux"),
        ("launchctl", "services on macOS"),
        ("diskutil", "drive health on macOS"),
        ("networksetup", "network kinds on macOS"),
        ("system_profiler", "hardware on macOS"),
    ];
    names
        .iter()
        .map(|(n, what)| {
            let at = find(n).map(|p| p.display().to_string()).unwrap_or_else(|| "not found".into());
            row(n, format!("{at} ({what})"))
        })
        .collect()
}

/// `name` on the PATH, with the extensions Windows runs.
fn find(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: &[&str] = if cfg!(windows) { &["", ".exe", ".cmd", ".bat"] } else { &[""] };
    std::env::split_paths(&path).find_map(|dir| exts.iter().map(|e| dir.join(format!("{name}{e}"))).find(|p| p.is_file()))
}

fn variables() -> Rows {
    ["UCHMK_CONFIG_DIR", "YAGURA_CONFIG_HOME", "YAGURA_STATE_HOME", "WGPU_BACKEND", "LANG", "LC_ALL"]
        .iter()
        .map(|k| row(k, std::env::var(k).unwrap_or_else(|_| "(unset)".into())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_key_gets_a_line_of_its_own() {
        let mut out = String::new();
        let long = "k".repeat(30);
        section(&mut out, "T", &[row("a", "1"), row(&long, "2\n3")]);
        assert_eq!(out, format!("T\n    a : 1\n    {long}\n        2\n        3\n\n"));
    }

    #[test]
    fn the_report_names_every_section_and_ends_in_one_newline() {
        let t = text();
        for title in ["Yagura\n", "Files\n", "Settings\n", "Tools\n", "Variables\n"] {
            assert!(t.contains(title), "{title}: {t}");
        }
        assert!(t.ends_with('\n') && !t.ends_with("\n\n"), "{t:?}");
        assert!(t.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn the_settings_say_what_was_read() {
        let s = Settings { check_updates: false, ..Settings::default() };
        let rows = settings_rows(&s);
        assert!(rows.contains(&row("check_updates", "off")));
        assert!(rows.contains(&row("agent allow", "nothing")));
    }
}
