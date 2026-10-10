//! What the settings screen reads and writes (ito's `docs/common-spec.md`):
//! common.toml (the language, the theme and the clock, shared by every uchmk
//! app), mimamori's own config.toml, and the user's themes. The files are
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
    /// The screen's page, search and focus.
    pub state: State,
    pub drafts: Drafts,
    tx: Sender<Reload>,
    pub rx: Receiver<Reload>,
    wake: Option<egui::Context>,
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
            state: State::default(),
            drafts: Drafts::new(),
            tx,
            rx,
            wake: None,
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

    /// Look at the files every 2 s from now on, and wake `ctx` when one
    /// has been read again.
    pub fn watch(&mut self, ctx: &egui::Context) {
        self.wake = Some(ctx.clone());
        let Some(base) = self.base.clone() else { return };
        let (common, themes, config) = (common_path(&base), themes_dir(&base), edit::config_path(&base));
        let mut paths = vec![common.clone(), config.clone(), themes.clone()];
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
            if changed.contains(&config) {
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
            let path = if file { edit::config_path(&base) } else { base.join("mimamori") };
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
        let os_light = ctx.system_theme() == Some(egui::Theme::Light);
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
        s.iter().chain(&self.common_warn).chain(&self.theme_warn).chain(&self.theme_err).cloned().collect()
    }
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
