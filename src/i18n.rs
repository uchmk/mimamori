//! The words on screen, looked up by key so another language is one more
//! table: every string the UI shows goes through [`tr`] (or [`trf`] with
//! arguments). English is built in (`src/lang/en.toml`); another language is
//! a file beside it (`ja.toml`), listed in [`AVAILABLE`], whose missing keys
//! fall back to English. The help's key descriptions come from the keymap,
//! in English; a table's `[keydesc]` translates the built-in ones
//! ([`tr_desc`]).
//!
//! Which language: the app's own `language` (mimamori's `config.toml`), else
//! the one in the `common.toml` all of uchmk's apps share, else the OS's
//! (`auto`). The table, the choice and the checks live in `tsumugi-i18n`,
//! shared with tsumugi (whose Settings window writes common.toml).

use std::sync::OnceLock;

use tsumugi_i18n::Table;

/// The languages with a table, English first.
pub const AVAILABLE: &[(&str, &str)] = &[("en", include_str!("lang/en.toml")), ("ja", include_str!("lang/ja.toml"))];

static TABLE: OnceLock<Table> = OnceLock::new();

/// Pick the table once, before the first frame. A later call changes nothing
/// (the language is read at startup, as filer reads its config).
pub fn set_language(code: &str) {
    let _ = TABLE.set(table(code));
}

fn table(code: &str) -> Table {
    let (t, errors) = Table::new(AVAILABLE, code);
    for e in errors {
        eprintln!("mimamori: {e}");
    }
    t
}

fn get() -> &'static Table {
    TABLE.get_or_init(|| table("en"))
}

/// The text for `key` in the chosen language; the key itself when no table
/// has it, so a missing entry shows up on screen instead of a blank.
pub fn tr(key: &str) -> &str {
    get().tr(key)
}

/// A key's description in the help: the chosen language's `[keydesc]` entry
/// for the keymap's English text, else that text (a user's own description
/// reads as written).
pub fn tr_desc(desc: &str) -> &str {
    get().tr_desc(desc)
}

/// [`tr`] with `{0}`, `{1}` … replaced by `args`.
pub fn trf(key: &str, args: &[&str]) -> String {
    get().trf(key, args)
}

/// The language to use: the first of the app's and the common setting that
/// is not `auto`, else the OS's (`ja-JP` → `ja`); anything without a table is
/// English.
pub fn resolve_language(app: Option<&str>, common: Option<&str>, os: Option<&str>) -> String {
    let codes: Vec<&str> = AVAILABLE.iter().map(|(l, _)| *l).collect();
    tsumugi_i18n::resolve(app, common, os, &codes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsumugi_i18n::check;

    #[test]
    fn a_missing_key_reads_as_itself() {
        assert_eq!(tr("no.such.key"), "no.such.key");
        assert_eq!(tr("status.paused"), "paused");
    }

    #[test]
    fn arguments_are_substituted() {
        assert_eq!(trf("toast.interval", &["2"]), "Interval 2 s");
    }

    #[test]
    fn the_app_setting_beats_the_common_one_and_auto_follows_the_os() {
        assert_eq!(resolve_language(Some("en"), Some("ja"), Some("ja-JP")), "en");
        assert_eq!(resolve_language(Some("auto"), Some("ja"), Some("en-US")), "ja");
        assert_eq!(resolve_language(Some("auto"), Some("en"), Some("ja-JP")), "en");
        assert_eq!(resolve_language(None, None, Some("ja-JP")), "ja");
        assert_eq!(resolve_language(None, None, Some("ja_JP.UTF-8")), "ja");
        assert_eq!(resolve_language(Some("JA"), None, None), "ja");
        assert_eq!(resolve_language(None, None, Some("en-US")), "en");
        assert_eq!(resolve_language(None, Some("auto"), None), "en");
        assert_eq!(resolve_language(Some("xx"), None, None), "en");
    }

    /// Every translation has each English key, no key English lacks (a typo
    /// would never be read), and the same `{0}` … to fill in.
    #[test]
    fn translations_match_english_key_for_key() {
        let bad = check::translations(AVAILABLE);
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }

    /// Each description in the built-in keymap has a `[keydesc]` entry in
    /// every translation.
    #[test]
    fn the_built_in_key_descriptions_are_translated() {
        let km: toml::Table = include_str!("defaults/keymap.toml").parse().unwrap();
        let mut descs = Vec::new();
        for layer in km.values().filter_map(toml::Value::as_table) {
            for list in layer.values().filter_map(toml::Value::as_array) {
                descs.extend(list.iter().filter_map(|b| b.get("desc")?.as_str()));
            }
        }
        assert!(descs.len() > 20, "read the keymap's descriptions");
        let bad = check::key_descriptions(AVAILABLE, &descs);
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }

    /// Every `tr("…")` / `trf("…"` in the source has an English entry, so no
    /// key ever shows on screen as itself.
    #[test]
    fn every_key_the_source_uses_has_english() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let missing = check::source_keys(&src, &table("en"), &["i18n.rs"]);
        assert!(missing.is_empty(), "keys without English:\n{}", missing.join("\n"));
    }
}
