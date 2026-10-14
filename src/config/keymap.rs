//! The keymap, laid out like yazi's and kura's: a section per layer, each
//! with `keymap` (replaces the built-in list), `prepend_keymap` and
//! `append_keymap`. The first binding that matches wins, so a prepended key
//! shadows a built-in one. Read by `ito-keymap`, kura's loader shared in
//! ito 0.32.0, so the two say the same about a key that can never run.

use super::cmd::{self, Act};
#[cfg(test)]
use super::keys::Key;

pub use ito_keymap::resolve;

pub type Binding = ito_keymap::Binding<Act>;
pub type Match<'a> = ito_keymap::Match<'a, Act>;

const DEFAULTS: &str = include_str!("../defaults/keymap.toml");

/// The layers: the screens, the filter's line, the help, the question
/// before a process is ended, the zoomed graph, the settings screen (where only these keys
/// are taken; the rest go to the screen's fields and lists).
#[derive(Default, Debug)]
pub struct Keymap {
    pub main: Vec<Binding>,
    pub input: Vec<Binding>,
    pub help: Vec<Binding>,
    pub confirm: Vec<Binding>,
    pub zoom: Vec<Binding>,
    pub settings: Vec<Binding>,
}

const SPEC: ito_keymap::Spec = ito_keymap::Spec {
    layers: &[
        ito_keymap::Layer::new("main"),
        ito_keymap::Layer::new("input"),
        ito_keymap::Layer::new("help"),
        ito_keymap::Layer::new("confirm"),
        ito_keymap::Layer::new("zoom"),
        ito_keymap::Layer::new("settings"),
    ],
    ignored: &[],
    defaults: DEFAULTS,
};

impl Keymap {
    /// The built-in keymap with the user's file (its text, if there is one)
    /// laid over it. Problems come back as warnings; a bad binding is skipped.
    /// A user's key that takes a built-in one's place is meant, and not among
    /// them.
    pub fn load(user: Option<&str>) -> (Self, Vec<String>) {
        let mut command = |s: &str, warn: &mut Vec<String>| match cmd::parse(s) {
            Ok(a) => Some(a),
            Err(e) => {
                warn.push(format!("keymap: {e}"));
                None
            }
        };
        let files: Vec<(&str, &str)> = user.map(|t| ("keymap.toml", t)).into_iter().collect();
        let mut km = ito_keymap::load(&SPEC, &files, &mut command);
        let mut take = |name| km.take(name);
        let out = Self {
            main: take("main"),
            input: take("input"),
            help: take("help"),
            confirm: take("confirm"),
            zoom: take("zoom"),
            settings: take("settings"),
        };
        (out, km.warnings)
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
        assert!(!km.main.is_empty() && !km.input.is_empty() && !km.help.is_empty() && !km.confirm.is_empty() && !km.zoom.is_empty());
        assert!(!km.settings.is_empty());
        assert!(matches!(resolve(&km.main, &keys("<C-,>")), Match::Exact(b) if b.run == [Act::Settings]) || cfg!(target_os = "macos"));
        assert!(matches!(resolve(&km.settings, &keys("<C-BackTab>")), Match::Exact(b) if b.run == [Act::Page(-1)]));
        assert!(matches!(resolve(&km.settings, &keys("<C-PageDown>")), Match::Exact(b) if b.run == [Act::Page(1)]));
    }

    #[test]
    fn resolves_exact_prefix_and_none() {
        let (km, _) = Keymap::load(None);
        assert!(matches!(resolve(&km.main, &keys("2")), Match::Exact(b) if b.run == [Act::Screen(Goto::To(Screen::Cpu))]));
        assert!(matches!(resolve(&km.main, &keys("g")), Match::Pending(_)));
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
        // Taking all of [help]'s keys away for one is said, in case it was
        // meant as a prepend; the `j` that takes a default's place is not.
        assert_eq!(warn.len(), 1, "{warn:?}");
        assert!(warn[0].contains("did you mean `[[help.prepend_keymap]]`"), "{warn:?}");
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
