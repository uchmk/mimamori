//! The settings files, all under one folder shared by uchmk's apps:
//!
//! ```text
//! <config dir>/uchmk/common.toml          language = "auto" | "en" | …
//! <config dir>/uchmk/mimamori/config.toml interval, language, backend
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
    pub warnings: Vec<String>,
}

/// The `uchmk` folder.
pub fn base_dir() -> Option<PathBuf> {
    match std::env::var_os("UCHMK_CONFIG_DIR") {
        Some(d) if !d.is_empty() => Some(PathBuf::from(d)),
        _ => dirs::config_dir().map(|d| d.join("uchmk")),
    }
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

/// Read everything from `base` (the `uchmk` folder; [`base_dir`] when
/// `None`). A missing file is the defaults; a bad value is warned about and
/// left at its default.
pub fn load(base: Option<&Path>) -> Settings {
    let base = base.map(Path::to_path_buf).or_else(base_dir);
    let mut warn = Vec::new();
    let mut s = Settings { interval: 1.0, language: "en".into(), backend: None, keymap: None, dir: None, warnings: Vec::new() };
    let mut common_lang = None;
    let mut app_lang = None;
    if let Some(base) = &base {
        if let Some(t) = read_table(&base.join("common.toml"), &mut warn) {
            common_lang = string(&t, "language", "common.toml", &mut warn);
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
                    other => warn.push(format!("config.toml: unknown setting `{other}`")),
                }
            }
        }
        s.keymap = std::fs::read_to_string(dir.join("keymap.toml")).ok();
        s.dir = Some(dir);
    }
    let os = sys_locale::get_locale();
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
        // "ja" has no table yet, so it falls back to English.
        assert_eq!(s.language, "en");
        assert_eq!(s.warnings.len(), 1, "{:?}", s.warnings);
        assert!(s.warnings[0].contains("colour"));

        std::fs::write(d.join("mimamori/config.toml"), "interval = 60\nbackend = \"glide\"\n").unwrap();
        let s = load(Some(&d));
        assert_eq!(s.interval, 1.0);
        assert!(s.backend.is_none());
        assert_eq!(s.warnings.len(), 2, "{:?}", s.warnings);
    }
}
