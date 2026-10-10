//! The words on screen, looked up by key so another language is one more
//! table: every string the UI shows goes through [`tr`] (or [`trf`] with
//! arguments). English is built in (`src/lang/en.toml`); a language added
//! later is a file beside it, listed in [`AVAILABLE`], whose missing keys fall
//! back to English.
//!
//! Which language: the app's own `language` (mimamori's `config.toml`), else
//! the one in the `common.toml` all of uchmk's apps share, else the OS's
//! (`auto`, through sys-locale). Meant to move into a crate shared with filer
//! and tsumugi once they read the same setting (TODO.md).

use std::collections::HashMap;
use std::sync::OnceLock;

/// The languages with a table, English first.
pub const AVAILABLE: &[(&str, &str)] = &[("en", include_str!("lang/en.toml"))];

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
        // Only English has a table so far, so a known code is "en"; what
        // the precedence picks shows through an unknown one turning to "en"
        // and "auto" passing the choice on.
        assert_eq!(resolve_language(Some("en"), Some("ja"), Some("ja-JP")), "en");
        assert_eq!(resolve_language(Some("auto"), Some("en"), Some("ja-JP")), "en");
        assert_eq!(resolve_language(None, None, Some("en-US")), "en");
        assert_eq!(resolve_language(None, Some("auto"), None), "en");
        assert_eq!(resolve_language(Some("xx"), None, None), "en");
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
