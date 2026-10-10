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
//! (`auto`, through sys-locale). Meant to move into a crate shared with filer
//! and tsumugi once they read the same setting (TODO.md).

use std::collections::HashMap;
use std::sync::OnceLock;

/// The languages with a table, English first.
pub const AVAILABLE: &[(&str, &str)] = &[("en", include_str!("lang/en.toml")), ("ja", include_str!("lang/ja.toml"))];

static TABLE: OnceLock<HashMap<String, &'static str>> = OnceLock::new();

/// Pick the table once, before the first frame. A later call changes nothing
/// (the language is read at startup, as filer reads its config).
pub fn set_language(code: &str) {
    let _ = TABLE.set(table(code));
}

fn table(code: &str) -> HashMap<String, &'static str> {
    let mut out = HashMap::new();
    // English underneath, so a key a translation lacks still reads.
    for (lang, src) in AVAILABLE {
        if *lang == "en" || *lang == code {
            flatten("", &parse(lang, src), &mut out);
        }
    }
    out
}

fn parse(lang: &str, src: &str) -> toml::Table {
    match src.parse::<toml::Table>() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("mimamori: lang/{lang}.toml: {e}");
            toml::Table::new()
        }
    }
}

/// `[status] paused = "…"` becomes `status.paused`. The strings live for the
/// whole run, so they are leaked once to hand out `&'static str`.
fn flatten(prefix: &str, t: &toml::Table, out: &mut HashMap<String, &'static str>) {
    for (k, v) in t {
        let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
        match v {
            toml::Value::String(s) => {
                out.insert(key, Box::leak(s.clone().into_boxed_str()));
            }
            toml::Value::Table(sub) => flatten(&key, sub, out),
            _ => {}
        }
    }
}

/// The text for `key` in the chosen language; the key itself when no table
/// has it, so a missing entry shows up on screen instead of a blank.
pub fn tr(key: &str) -> &str {
    let t = TABLE.get_or_init(|| table("en"));
    t.get(key).copied().unwrap_or(key)
}

/// A key's description in the help: the chosen language's `[keydesc]` entry
/// for the keymap's English text, else that text (a user's own description
/// reads as written).
pub fn tr_desc(desc: &str) -> &str {
    let t = TABLE.get_or_init(|| table("en"));
    t.get(&format!("keydesc.{desc}")).copied().unwrap_or(desc)
}

/// [`tr`] with `{0}`, `{1}` … replaced by `args`.
pub fn trf(key: &str, args: &[&str]) -> String {
    let mut s = tr(key).to_string();
    for (i, a) in args.iter().enumerate() {
        s = s.replace(&format!("{{{i}}}"), a);
    }
    s
}

/// The language to use: the first of the app's and the common setting that
/// is not `auto`, else the OS's (`ja-JP` → `ja`); anything without a table is
/// English.
pub fn resolve_language(app: Option<&str>, common: Option<&str>, os: Option<&str>) -> String {
    let chosen = [app, common].into_iter().flatten().map(str::trim).find(|l| !l.is_empty() && !l.eq_ignore_ascii_case("auto"));
    let code = match chosen {
        Some(l) => l.to_ascii_lowercase(),
        None => os.and_then(|l| l.split(['-', '_']).next()).unwrap_or("en").to_ascii_lowercase(),
    };
    if AVAILABLE.iter().any(|(l, _)| *l == code) { code } else { "en".into() }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn own(code: &str) -> HashMap<String, &'static str> {
        let src = AVAILABLE.iter().find(|(l, _)| *l == code).unwrap().1;
        let mut out = HashMap::new();
        flatten("", &src.parse::<toml::Table>().unwrap(), &mut out);
        out
    }

    /// The `{0}` … a string takes, in order of number.
    fn holes(s: &str) -> Vec<usize> {
        let mut v: Vec<usize> = (0..10).filter(|i| s.contains(&format!("{{{i}}}"))).collect();
        v.sort();
        v
    }

    /// Every translation has each English key, no key English lacks (a typo
    /// would never be read), and the same `{0}` … to fill in.
    #[test]
    fn translations_match_english_key_for_key() {
        let en = own("en");
        for (code, _) in AVAILABLE.iter().filter(|(l, _)| *l != "en") {
            let t = own(code);
            let mut bad: Vec<String> = en.keys().filter(|k| !t.contains_key(*k)).map(|k| format!("missing {k}")).collect();
            for (k, v) in &t {
                if k.starts_with("keydesc.") {
                    continue;
                }
                match en.get(k) {
                    None => bad.push(format!("not in English: {k}")),
                    Some(e) if holes(e) != holes(v) => bad.push(format!("{k}: {:?} vs {:?}", holes(e), holes(v))),
                    _ => {}
                }
            }
            bad.sort();
            assert!(bad.is_empty(), "{code}.toml:\n{}", bad.join("\n"));
        }
    }

    /// Each description in the built-in keymap has a `[keydesc]` entry in
    /// every translation.
    #[test]
    fn the_built_in_key_descriptions_are_translated() {
        let km: toml::Table = include_str!("defaults/keymap.toml").parse().unwrap();
        let mut descs = Vec::new();
        for layer in km.values().filter_map(toml::Value::as_table) {
            for list in layer.values().filter_map(toml::Value::as_array) {
                descs.extend(list.iter().filter_map(|b| b.get("desc")?.as_str().map(str::to_string)));
            }
        }
        assert!(descs.len() > 20, "read the keymap's descriptions");
        for (code, _) in AVAILABLE.iter().filter(|(l, _)| *l != "en") {
            let t = own(code);
            let missing: Vec<&String> = descs.iter().filter(|d| !t.contains_key(&format!("keydesc.{d}"))).collect();
            assert!(missing.is_empty(), "{code}.toml [keydesc] lacks {missing:?}");
        }
    }

    /// Every `tr("…")` / `trf("…"` in the source has an English entry, so no
    /// key ever shows on screen as itself.
    #[test]
    fn every_key_the_source_uses_has_english() {
        let en = table("en");
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut missing = Vec::new();
        let mut stack = vec![src];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                if p.extension().is_none_or(|x| x != "rs") || p.ends_with("i18n.rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&p).unwrap();
                for pat in ["tr(\"", "trf(\""] {
                    for (i, _) in text.match_indices(pat) {
                        // `xtr(` or `.tr(` would be someone else's function.
                        if text[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_') {
                            continue;
                        }
                        let rest = &text[i + pat.len()..];
                        let key = &rest[..rest.find('"').unwrap()];
                        if !en.contains_key(key) {
                            missing.push(format!("{}: {key}", p.display()));
                        }
                    }
                }
            }
        }
        assert!(missing.is_empty(), "keys without English:\n{}", missing.join("\n"));
    }
}
