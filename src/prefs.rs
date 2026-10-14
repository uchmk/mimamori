//! What the settings screen reads and writes (ito's `docs/common-spec.md`):
//! common.toml (the language, the theme and the clock, shared by every uchmk
//! app), yagura's own config.toml, and the user's themes. The files are
//! watched every 2 s and written on threads of their own; what was read
//! comes back as a [`Reload`], which `App::pump` folds in.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use ito_common::{Common, CommonChange, common_path, edit_common, themes_dir, write_atomic};
use ito_prefs::{Drafts, State};
use ito_theme::Theme;

use crate::config::edit::{self, Edit};
use crate::config::settings::{self, Settings};
use crate::config::theme;

/// How often the files are looked at.
const WATCH: Duration = Duration::from_secs(2);

/// A file read again, or a write that failed.
pub enum Reload {
    Common(Result<(Common, Vec<String>), String>),
    Themes(Vec<Theme>, Vec<String>),
    Settings(Box<Settings>),
    /// The Keys page wrote keymap.toml (or found it already said so):
    /// config.toml and keymap.toml read again, and what to say.
    Keys(Box<Settings>, String),
    /// This machine's public key and name, or why it could not be read.
    Key(Result<(String, String), String>),
    Failed(String),
}

pub struct Prefs {
    /// The `uchmk` folder; `None` when the OS has no config directory.
    pub base: Option<PathBuf>,
    pub common: Common,
    /// The built-in themes, then the user's.
    pub themes: Vec<Theme>,
    /// config.toml as last read.
    pub settings: Settings,
    pub common_warn: Vec<String>,
    pub theme_warn: Vec<String>,
    /// The theme common.toml names but which is not there.
    pub theme_err: Option<String>,
    /// What reading keymap.toml said, last time it was read.
    pub keymap_warn: Vec<String>,
    /// The screen's page, search and focus.
    pub state: State,
    pub drafts: Drafts,
    /// The Keys page's search and the key being taken.
    pub keys_edit: ito_prefs::KeysEdit,
    /// This machine's public key (base64) and name for the fleet page;
    /// `None` until read.
    pub key: Option<Result<(String, String), String>>,
    key_asked: bool,
    tx: Sender<Reload>,
    pub rx: Receiver<Reload>,
    wake: Option<egui::Context>,
    /// The OS's light or dark, for `system` (egui's lags a switch on Windows).
    os_light: ito_common::OsLight,
}

impl Prefs {
    /// Nothing read: the defaults and the built-in themes (the tests).
    pub fn empty() -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        Self {
            base: None,
            common: Common::default(),
            themes: ito_theme::builtin(),
            settings: Settings::default(),
            common_warn: Vec::new(),
            theme_warn: Vec::new(),
            theme_err: None,
            keymap_warn: Vec::new(),
            state: State::default(),
            drafts: Drafts::new(),
            keys_edit: ito_prefs::KeysEdit::default(),
            key: None,
            key_asked: false,
            tx,
            rx,
            wake: None,
            os_light: ito_common::OsLight::default(),
        }
    }

    /// common.toml and the themes in `base`, with config.toml as `settings`
    /// already read it.
    pub fn load(base: Option<PathBuf>, settings: Settings) -> Self {
        let mut p = Self { settings, ..Self::empty() };
        if let Some(b) = &base {
            match Common::read(b) {
                Ok((c, w)) => (p.common, p.common_warn) = (c, w),
                Err(e) => p.common_warn = vec![e],
            }
            let (user, warn) = ito_theme::load_dir(&themes_dir(b));
            p.themes.extend(user);
            p.theme_warn = warn;
        }
        p.base = base;
        p
    }

    /// Look at the files and the OS's light or dark every 2 s from now on,
    /// and wake `ctx` when one has been read again.
    pub fn watch(&mut self, ctx: &egui::Context) {
        self.wake = Some(ctx.clone());
        let os_ctx = ctx.clone();
        self.os_light = ito_common::OsLight::watch(WATCH, move || os_ctx.request_repaint());
        let Some(base) = self.base.clone() else { return };
        let (common, themes, config) = (common_path(&base), themes_dir(&base), edit::config_path(&base));
        let keymap = keymap_path(&base);
        let mut paths = vec![common.clone(), config.clone(), keymap.clone(), themes.clone()];
        // A theme file edited in place; one added or taken away changes the
        // folder's time.
        if let Ok(entries) = std::fs::read_dir(&themes) {
            paths.extend(entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "toml")));
        }
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        ito_common::watch(paths, WATCH, move |changed| {
            let mut sent = true;
            if changed.contains(&common) {
                sent &= tx.send(Reload::Common(Common::read(&base))).is_ok();
            }
            if changed.contains(&config) || changed.contains(&keymap) {
                sent &= tx.send(Reload::Settings(Box::new(settings::load(Some(&base))))).is_ok();
            }
            if changed.iter().any(|p| p.starts_with(&themes)) {
                let (user, warn) = ito_theme::load_dir(&themes);
                let all = ito_theme::builtin().into_iter().chain(user).collect();
                sent &= tx.send(Reload::Themes(all, warn)).is_ok();
            }
            ctx.request_repaint();
            sent
        });
    }

    fn send_back(tx: &Sender<Reload>, wake: &Option<egui::Context>, r: Reload) {
        let _ = tx.send(r);
        if let Some(ctx) = wake {
            ctx.request_repaint();
        }
    }

    /// Write `changes` into common.toml on a thread, then read it again.
    pub fn common_change(&self, changes: Vec<CommonChange>) {
        if changes.is_empty() {
            return;
        }
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        let Some(base) = self.base.clone() else {
            Self::send_back(&tx, &wake, Reload::Failed(crate::i18n::tr("settings.no_folder").into()));
            return;
        };
        std::thread::spawn(move || {
            let done = edit_common(&base, |text| changes.iter().try_fold(text.to_owned(), |t, c| c.apply(&t)));
            let r = match done {
                Ok(()) => Reload::Common(Common::read(&base)),
                Err(e) => Reload::Failed(e),
            };
            Self::send_back(&tx, &wake, r);
        });
    }

    /// Write one change into config.toml on a thread, then read it again.
    pub fn config_edit(&self, e: Edit) {
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        let Some(base) = self.base.clone() else {
            Self::send_back(&tx, &wake, Reload::Failed(crate::i18n::tr("settings.no_folder").into()));
            return;
        };
        std::thread::spawn(move || {
            let r = match edit::save(&base, &e) {
                Ok(()) => Reload::Settings(Box::new(settings::load(Some(&base)))),
                Err(e) => Reload::Failed(e),
            };
            Self::send_back(&tx, &wake, r);
        });
    }

    /// Make the Keys page's `edits` in keymap.toml on a thread (only what
    /// differs from the built-in keys is written), then read the files again.
    pub fn keymap_edit(&self, edits: Vec<ito_keymap::Edit>) {
        if edits.is_empty() {
            return;
        }
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        let Some(base) = self.base.clone() else {
            Self::send_back(&tx, &wake, Reload::Failed(crate::i18n::tr("settings.no_folder").into()));
            return;
        };
        std::thread::spawn(move || {
            let r = match write_keymap(&keymap_path(&base), &edits) {
                Ok(said) => Reload::Keys(Box::new(settings::load(Some(&base))), said),
                Err(e) => Reload::Failed(e),
            };
            Self::send_back(&tx, &wake, r);
        });
    }

    /// Read this machine's key on a thread, made the first time, once: the
    /// fleet page asks when it is first shown.
    pub fn ask_key(&mut self) {
        if std::mem::replace(&mut self.key_asked, true) {
            return;
        }
        let (tx, wake, dir) = (self.tx.clone(), self.wake.clone(), self.settings.dir.clone());
        let Some(dir) = dir else {
            Self::send_back(&tx, &wake, Reload::Key(Err(crate::i18n::tr("settings.no_folder").into())));
            return;
        };
        std::thread::spawn(move || {
            let r = crate::remote::noise::load_or_make(&dir)
                .map(|k| (crate::remote::noise::encode(&k.public), crate::remote::noise::host_name()))
                .map_err(|e| e.to_string());
            Self::send_back(&tx, &wake, Reload::Key(r));
        });
    }

    /// config.toml's full path, when there is a config folder.
    pub fn config_path(&self) -> Option<PathBuf> {
        self.base.as_deref().map(edit::config_path)
    }

    /// Open config.toml (made empty first when there is none) or a folder
    /// with what the OS opens it with, on a thread.
    pub fn open(&self, file: bool) {
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        let Some(base) = self.base.clone() else {
            Self::send_back(&tx, &wake, Reload::Failed(crate::i18n::tr("settings.no_folder").into()));
            return;
        };
        std::thread::spawn(move || {
            let path = if file { edit::config_path(&base) } else { crate::config::settings::own_dir(&base) };
            let made = if file && !path.exists() {
                write_atomic(&path, "")
            } else if !file {
                std::fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))
            } else {
                Ok(())
            };
            if let Err(e) = made.and_then(|()| open_path(&path)) {
                Self::send_back(&tx, &wake, Reload::Failed(e));
            }
        });
    }

    /// The theme common.toml picks, in force from this frame on: egui's
    /// widgets follow when it changes. Called every frame (the OS may have
    /// gone light or dark).
    pub fn apply_theme(&mut self, ctx: &egui::Context) {
        let os_light = self.os_light.light_or(ctx.system_theme() == Some(egui::Theme::Light));
        let (choice, dark, light) = self.common.theme_choice(None);
        let colors = match ito_theme::pick(&self.themes, &choice, &dark, &light, os_light) {
            Ok(t) => {
                self.theme_err = None;
                t.colors
            }
            Err(e) => {
                self.theme_err = Some(e);
                self.themes[0].colors
            }
        };
        if colors != ito_theme::colors() {
            ito_theme::set(colors);
            ctx.set_visuals(theme::visuals());
        }
    }

    /// Every warning from reading the files, for the Advanced page.
    pub fn warnings(&self) -> Vec<String> {
        let s = &self.settings.warnings;
        s.iter().chain(&self.keymap_warn).chain(&self.common_warn).chain(&self.theme_warn).chain(&self.theme_err).cloned().collect()
    }
}

/// keymap.toml's full path: beside config.toml, in yagura's own folder.
pub fn keymap_path(base: &Path) -> PathBuf {
    settings::own_dir(base).join("keymap.toml")
}

/// Make `edits` in the keymap.toml at `path` (none yet is an empty one),
/// written only when that changes it; what to say about it. Call it off the
/// UI thread.
pub fn write_keymap(path: &Path, edits: &[ito_keymap::Edit]) -> Result<String, String> {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let at = |e: String| format!("{}: {e}", path.display());
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(at(e.to_string())),
    };
    let new = ito_keymap::edit(&text, &crate::config::keymap::SPEC, edits, "noop").map_err(at)?;
    let what = edits
        .iter()
        .map(|e| match e {
            ito_keymap::Edit::Add { on, run, .. } => crate::i18n::trf("toast.key_added", &[&crate::config::keys::display_seq(on), run]),
            ito_keymap::Edit::Remove { on, run, .. } => {
                crate::i18n::trf("toast.key_removed", &[&crate::config::keys::display_seq(on), run])
            }
            ito_keymap::Edit::Reset { run, .. } => crate::i18n::trf("toast.key_reset", &[run]),
        })
        .collect::<Vec<_>>()
        .join(", ");
    if new == text {
        return Ok(crate::i18n::trf("toast.keymap_same", &[&what]));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| at(e.to_string()))?;
    }
    write_atomic(path, new)?;
    Ok(crate::i18n::trf("toast.keymap_saved", &[&what]))
}

#[cfg(windows)]
fn opener() -> std::process::Command {
    std::process::Command::new("explorer")
}

#[cfg(target_os = "macos")]
fn opener() -> std::process::Command {
    std::process::Command::new("open")
}

#[cfg(not(any(windows, target_os = "macos")))]
fn opener() -> std::process::Command {
    std::process::Command::new("xdg-open")
}

fn open_path(path: &Path) -> Result<(), String> {
    // explorer answers 1 even when it opened the file, so only a failure to
    // start it is said; waited for, so it leaves nothing behind.
    opener().arg(path).status().map(drop).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ito_keymap::Edit;

    fn on(s: &str) -> Vec<crate::config::keys::Key> {
        s.split(' ').map(|k| crate::config::keys::Key::parse(k).expect("a key")).collect()
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).expect("keymap.toml written")
    }

    #[test]
    fn a_new_key_writes_only_that_entry() {
        let path = keymap_path(&crate::util::test_dir("keymap-add"));
        let add = Edit::Add { layer: "main".into(), on: on("<C-S-Enter>"), run: "quit".into(), desc: "Quit".into() };
        write_keymap(&path, &[add]).expect("written");
        let text = read(&path);
        assert!(text.contains("prepend_keymap"), "{text}");
        assert!(text.contains("<C-S-Enter>") && text.contains(r#"run = "quit""#), "{text}");
        // Nothing of the defaults is copied in.
        assert!(!text.contains("screen overview") && !text.contains(r#""q""#), "{text}");
        let (km, warn) = crate::config::keymap::Keymap::load(Some(&text));
        assert!(warn.is_empty(), "{warn:?}");
        let main = km.layers().into_iter().find(|(l, _)| l == "main").expect("main").1;
        assert!(crate::config::keymap::live(&main).any(|b| b.on == on("<C-S-Enter>")));
    }

    #[test]
    fn taking_a_default_off_writes_noop_and_a_reset_leaves_nothing() {
        let path = keymap_path(&crate::util::test_dir("keymap-remove"));
        let remove = Edit::Remove { layer: "main".into(), on: on("q"), run: "quit".into() };
        write_keymap(&path, &[remove]).expect("written");
        let text = read(&path);
        assert!(text.contains(r#"on = "q""#) && text.contains(r#"run = "noop""#), "{text}");
        let add = Edit::Add { layer: "main".into(), on: on("<C-S-Enter>"), run: "quit".into(), desc: "Quit".into() };
        write_keymap(&path, &[add]).expect("written");
        write_keymap(&path, &[Edit::Reset { layer: "main".into(), run: "quit".into() }]).expect("written");
        let text = read(&path);
        assert!(!text.contains("noop") && !text.contains("<C-S-Enter>"), "{text}");
    }

    #[test]
    fn edits_that_change_nothing_make_no_file() {
        let path = keymap_path(&crate::util::test_dir("keymap-same"));
        write_keymap(&path, &[Edit::Reset { layer: "main".into(), run: "quit".into() }]).expect("nothing to do");
        assert!(!path.exists());
    }
}
