//! The keymap, laid out like yazi's and filer's: a section per layer, each
//! with `keymap` (replaces the built-in list), `prepend_keymap` and
//! `append_keymap`. The first binding that matches wins, so a prepended key
//! shadows a built-in one.

use serde::Deserialize;

use super::cmd::{self, Act};
use super::keys::Key;

const DEFAULTS: &str = include_str!("../defaults/keymap.toml");

#[derive(Clone, Debug)]
pub struct Binding {
    pub on: Vec<Key>,
    pub run: Vec<Act>,
    pub desc: String,
}

/// The layers: the screens, the filter's line, the help, the question
/// before a process is ended, the settings screen (where only these keys
/// are taken; the rest go to the screen's fields and lists).
#[derive(Default, Debug)]
pub struct Keymap {
    pub main: Vec<Binding>,
    pub input: Vec<Binding>,
    pub help: Vec<Binding>,
    pub confirm: Vec<Binding>,
    pub settings: Vec<Binding>,
}

pub enum Match<'a> {
    Exact(&'a Binding),
    /// More keys are needed.
    Prefix,
    None,
}

pub fn resolve<'a>(layer: &'a [Binding], seq: &[Key]) -> Match<'a> {
    let mut longer = false;
    for b in layer {
        if b.on.len() >= seq.len() && b.on[..seq.len()] == *seq {
            if b.on.len() == seq.len() {
                return Match::Exact(b);
            }
            longer = true;
        }
    }
    if longer { Match::Prefix } else { Match::None }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    main: Section,
    #[serde(default)]
    input: Section,
    #[serde(default)]
    help: Section,
    #[serde(default)]
    confirm: Section,
    #[serde(default)]
    settings: Section,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Section {
    keymap: Option<Vec<Entry>>,
    #[serde(default)]
    prepend_keymap: Vec<Entry>,
    #[serde(default)]
    append_keymap: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    fn items(&self) -> Vec<&str> {
        match self {
            Self::One(s) => s.split_whitespace().collect(),
            Self::Many(v) => v.iter().map(String::as_str).collect(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    on: OneOrMany,
    /// What `on` is on macOS, where the platform's chords differ (tsumugi's
    /// resize is Cmd+Ctrl+arrow there).
    on_mac: Option<OneOrMany>,
    run: OneOrMany,
    #[serde(default)]
    desc: String,
}

impl Entry {
    fn binding(&self, warn: &mut Vec<String>) -> Option<Binding> {
        let on = match (&self.on_mac, cfg!(target_os = "macos")) {
            (Some(mac), true) => mac,
            _ => &self.on,
        };
        let mut keys = Vec::new();
        for k in on.items() {
            match Key::parse(k) {
                Some(key) => keys.push(key),
                None => {
                    warn.push(format!("keymap: bad key `{k}`"));
                    return None;
                }
            }
        }
        if keys.is_empty() {
            warn.push("keymap: a binding with no key".into());
            return None;
        }
        let mut run = Vec::new();
        for c in self.run.items_as_commands() {
            match cmd::parse(c) {
                Ok(a) => run.push(a),
                Err(e) => {
                    warn.push(format!("keymap: {e}"));
                    return None;
                }
            }
        }
        Some(Binding { on: keys, run, desc: self.desc.clone() })
    }
}

impl OneOrMany {
    /// `run` is one command per string, not split on spaces.
    fn items_as_commands(&self) -> Vec<&str> {
        match self {
            Self::One(s) => vec![s.as_str()],
            Self::Many(v) => v.iter().map(String::as_str).collect(),
        }
    }
}

fn parse_file(src: &str, what: &str, warn: &mut Vec<String>) -> File {
    match toml::from_str(src) {
        Ok(f) => f,
        Err(e) => {
            warn.push(format!("{what}: {e}"));
            File::default()
        }
    }
}

fn layer(default: &Section, user: &Section, warn: &mut Vec<String>) -> Vec<Binding> {
    let base = user.keymap.as_ref().or(default.keymap.as_ref());
    let all = user.prepend_keymap.iter().chain(base.into_iter().flatten()).chain(&user.append_keymap);
    all.filter_map(|e| e.binding(warn)).collect()
}

impl Keymap {
    /// The built-in keymap with the user's file (its text, if there is one)
    /// laid over it. Problems come back as warnings; a bad binding is skipped.
    pub fn load(user: Option<&str>) -> (Self, Vec<String>) {
        let mut warn = Vec::new();
        let d = parse_file(DEFAULTS, "built-in keymap", &mut warn);
        let u = user.map(|s| parse_file(s, "keymap.toml", &mut warn)).unwrap_or_default();
        let km = Self {
            main: layer(&d.main, &u.main, &mut warn),
            input: layer(&d.input, &u.input, &mut warn),
            help: layer(&d.help, &u.help, &mut warn),
            confirm: layer(&d.confirm, &u.confirm, &mut warn),
            settings: layer(&d.settings, &u.settings, &mut warn),
        };
        (km, warn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::cmd::{Goto, Step};
    use crate::screen::Screen;

    fn keys(s: &str) -> Vec<Key> {
        s.split_whitespace().map(|k| Key::parse(k).unwrap()).collect()
    }

    #[test]
    fn the_defaults_parse_without_a_warning() {
        let (km, warn) = Keymap::load(None);
        assert!(warn.is_empty(), "{warn:?}");
        assert!(!km.main.is_empty() && !km.input.is_empty() && !km.help.is_empty() && !km.confirm.is_empty());
        assert!(!km.settings.is_empty());
        assert!(matches!(resolve(&km.main, &keys("<C-,>")), Match::Exact(b) if b.run == [Act::Settings]) || cfg!(target_os = "macos"));
        assert!(matches!(resolve(&km.settings, &keys("<C-BackTab>")), Match::Exact(b) if b.run == [Act::Page(-1)]));
        assert!(matches!(resolve(&km.settings, &keys("<C-PageDown>")), Match::Exact(b) if b.run == [Act::Page(1)]));
    }

    #[test]
    fn resolves_exact_prefix_and_none() {
        let (km, _) = Keymap::load(None);
        assert!(matches!(resolve(&km.main, &keys("2")), Match::Exact(b) if b.run == [Act::Screen(Goto::To(Screen::Cpu))]));
        assert!(matches!(resolve(&km.main, &keys("g")), Match::Prefix));
        assert!(matches!(resolve(&km.main, &keys("g g")), Match::Exact(b) if b.run == [Act::Arrow(Step::Top)]));
        assert!(matches!(resolve(&km.main, &keys("<F9>")), Match::None));
    }

    #[test]
    fn a_prepended_key_wins_and_keymap_replaces() {
        let user = r#"
            [main]
            prepend_keymap = [{ on = "j", run = "quit" }]
            [help]
            keymap = [{ on = ["x"], run = "escape" }]
        "#;
        let (km, warn) = Keymap::load(Some(user));
        assert!(warn.is_empty(), "{warn:?}");
        assert!(matches!(resolve(&km.main, &keys("j")), Match::Exact(b) if b.run == [Act::Quit]));
        assert_eq!(km.help.len(), 1);
    }

    #[test]
    fn a_bad_binding_is_skipped_with_a_warning() {
        let user = r#"
            [main]
            append_keymap = [{ on = "<Nope>", run = "quit" }, { on = "z", run = "fly" }]
        "#;
        let (km, warn) = Keymap::load(Some(user));
        assert_eq!(warn.len(), 2, "{warn:?}");
        assert!(matches!(resolve(&km.main, &keys("z")), Match::None));
    }
}
