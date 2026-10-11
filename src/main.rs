//! yagura: a keyboard-driven system monitor, the sibling of kura and
//! tsumugi. The window and the input; the state is in `app.rs`, the
//! drawing in `ui/`, the sampling in `collect/`.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod collect;
mod config;
mod export;
mod fonts;
mod gpu_backend;
mod i18n;
mod icon;
mod keyscript;
mod kill;
mod model;
mod persist;
mod prefs;
mod remote;
mod screen;
mod service;
mod stacks;
mod svcctl;
mod tree;
mod ui;
mod util;
#[cfg(test)]
mod winres;

use std::path::PathBuf;
use std::time::Duration;

use app::{App, Mode};
use config::cmd::Act;
use config::keys::{Key, from_egui};
use config::{keymap::Keymap, settings, theme};
use keyscript::{Do, Runner};

/// The window's icon, in pixels square.
const ICON_PX: u32 = 128;

const USAGE: &str = "\
yagura — a keyboard-driven system monitor

usage: yagura [--keys SCRIPT]
       yagura --agent [--log FILE]
       yagura --snapshot
       yagura --log-metrics FILE [--every SECONDS]
       yagura --install-service | --uninstall-service
       yagura --key

  --keys SCRIPT  press these keys once the window is up, as kura's --keys
                 (`<Wait:ms>`, `<Shot:name>`, `<State:name>`, `<Click:x,y>`,
                 `<Drag:x,y-x,y>`, `<Quit>`); YAGURA_KEYS_DONE names the
                 file the final state is written to
  --agent        no window: serve this machine's summary to the fleet screen
                 of other machines, on 127.0.0.1:7313 only (`[agent] port`);
                 reach it through `ssh -L 7313:localhost:7313 HOST` or
                 `tailscale serve --bg --tcp 7313 tcp://localhost:7313`.
                 View only unless `[agent] allow` says otherwise. With
                 `[agent] listen = \"0.0.0.0:7314\"` it also takes viewers
                 whose keys `[[agent.viewer]]` lists, directly, over Noise
  --log FILE     with --agent: append what the agent says to FILE as well
                 (Windows has no console to say it to when started at logon)
  --snapshot     no window: read everything once and print it as JSON (the
                 system, CPU, memory, processes, GPUs, services, network,
                 disks, sensors, battery, hardware and devices)
  --log-metrics FILE
                 no window: append a CSV row of the main numbers (CPU,
                 load, memory, network, disk, GPU, hottest temperature,
                 battery, process count) to FILE every --every seconds
                 (default 5) until stopped with Ctrl+C
  --every SECONDS
                 the gap between rows of --log-metrics
  --install-service
                 start the agent at logon from now on, and now: a systemd
                 user unit on Linux, a LaunchAgent on macOS, the user's Run
                 key on Windows. Needs no administrator
  --uninstall-service
                 stop that agent and remove what --install-service added
  --key          print this machine's public key (made the first time) and
                 what to add to the other machine's config.toml for a direct
                 connection
  --version      print the version
  --help         print this

Settings: Ctrl+, (Cmd+, on macOS) opens the settings screen. The files are
<config dir>/uchmk/yagura/config.toml ([[host]] lists the fleet) and keymap.toml;
the language, the scale, the theme and the clock are shared by every uchmk app in
<config dir>/uchmk/common.toml, and themes of your own go in <config dir>/uchmk/themes/.

Environment: UCHMK_CONFIG_DIR moves the whole uchmk folder, YAGURA_CONFIG_HOME
yagura's own folder and YAGURA_STATE_HOME the folder of state.toml (the column
widths).";

struct Yagura {
    app: App,
    script: Option<Runner>,
    /// What the script asked for in `raw_input_hook`, done in the frame.
    todo: Vec<Do>,
}

impl Yagura {
    fn write_state(&self, path: PathBuf, tail: &str) {
        let text = format!("{}{tail}", self.app.report());
        if let Err(e) = std::fs::write(&path, text) {
            eprintln!("yagura --keys: {}: {e}", path.display());
        }
    }

    /// The settings screen's keys, from its layer of the keymap: those
    /// taken out of egui's input, the rest left to the controls.
    fn settings_keys(&mut self, ctx: &egui::Context) -> ito_prefs::Keys {
        let mut keys = ito_prefs::Keys::default();
        // A control had the keys: Esc is its (egui lets go of the control).
        let busy = self.app.prefs.state.held || ctx.memory(|m| m.focused().is_some());
        let events = ctx.input(|i| i.events.clone());
        let mut used = Vec::new();
        for (n, e) in events.iter().enumerate() {
            let egui::Event::Key { key, pressed: true, modifiers, .. } = e else { continue };
            let Some(k) = from_egui(*key, modifiers) else { continue };
            match self.app.settings_act(k) {
                Some(Act::Escape) if busy => continue,
                Some(Act::Escape | Act::Settings) => keys.close = true,
                Some(Act::Page(d)) if d < 0 => keys.back = true,
                Some(Act::Page(_)) => keys.next = true,
                Some(Act::Search) => keys.find = true,
                Some(Act::Scale(to)) => self.app.scale(to),
                _ => continue,
            }
            used.push(n);
        }
        if !used.is_empty() {
            ctx.input_mut(|i| {
                let mut n = 0;
                i.events.retain(|_| {
                    n += 1;
                    !used.contains(&(n - 1))
                });
            });
        }
        keys
    }

    fn keys(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|i| i.events.clone());
        for e in events {
            match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    if let Some(k) = from_egui(key, &modifiers) {
                        self.app.feed(k);
                    }
                }
                egui::Event::Text(t) => {
                    for c in t.chars() {
                        self.app.feed(Key::char(c));
                    }
                }
                _ => {}
            }
        }
    }

    fn script(&mut self, ctx: &egui::Context) {
        let Some(script) = &mut self.script else { return };
        if let Some(err) = script.shot(ctx) {
            self.app.toast(err);
        }
        for d in std::mem::take(&mut self.todo) {
            let Some(script) = &self.script else { return };
            match d {
                Do::State(name) => self.write_state(script.dir.join(format!("{name}.txt")), ""),
                Do::Quit => self.app.quit = true,
                Do::Done => {
                    if let Some(done) = script.done.clone() {
                        self.write_state(done, "keys: done\n");
                    }
                }
            }
        }
        if let Some(due) = self.script.as_ref().and_then(Runner::due) {
            ctx.request_repaint_after(due);
        }
    }
}

impl eframe::App for Yagura {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some(script) = &mut self.script
            && let Some(d) = script.input(ctx, raw)
        {
            self.todo.push(d);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let keys = if self.app.mode == Mode::Settings { self.settings_keys(&ctx) } else { ito_prefs::Keys::default() };
        if self.app.mode != Mode::Settings {
            self.keys(&ctx);
        }
        self.script(&ctx);
        self.app.pump();
        // common.toml's theme, or the OS's light or dark under `system`.
        self.app.prefs.apply_theme(&ctx);
        if self.app.mode == Mode::Settings {
            ui::settings::show(ui, &mut self.app, keys);
        } else {
            let out = ui::frame(ui, &self.app);
            self.app.apply_out(out, &ctx);
        }
        if let Some(text) = self.app.copy.take() {
            ctx.copy_text(text);
        }
        if self.app.quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // One more frame to take the toast away; nothing else needs a frame
        // until the sampler sends one.
        if let Some(left) = self.app.toast_left() {
            ctx.request_repaint_after(left);
        }
        if let Some(left) = self.app.probe_agent() {
            ctx.request_repaint_after(left);
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        theme::bg().to_normalized_gamma_f32()
    }
}

/// A release build on Windows has no console of its own: borrow the one it
/// was started from, so `--help`, `--version` and `--agent` can be read.
#[cfg(windows)]
fn attach_console() {
    use ::windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: no arguments to get wrong; failing (no parent console) is fine.
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

#[cfg(not(windows))]
fn attach_console() {}

/// This machine's name, for `--key`'s examples.
fn main() -> eframe::Result {
    let mut args = std::env::args().skip(1).peekable();
    if args.peek().is_some() {
        attach_console();
    }
    let mut script = None;
    let mut agent = false;
    let mut log = None;
    let mut service = None;
    let mut show_key = false;
    let mut snapshot = false;
    let mut metrics: Option<PathBuf> = None;
    let mut every = 5.0_f64;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--version" | "-V" => {
                println!("yagura {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            "--agent" => agent = true,
            "--log" => {
                let Some(p) = args.next() else {
                    eprintln!("yagura: --log needs a file");
                    std::process::exit(2);
                };
                log = Some(PathBuf::from(p));
            }
            "--snapshot" => snapshot = true,
            "--log-metrics" => {
                let Some(p) = args.next() else {
                    eprintln!("yagura: --log-metrics needs a file");
                    std::process::exit(2);
                };
                metrics = Some(PathBuf::from(p));
            }
            "--every" => match args.next().and_then(|v| v.parse::<f64>().ok()).filter(|v| *v >= 1.0 && v.is_finite()) {
                Some(v) => every = v,
                None => {
                    eprintln!("yagura: --every needs a number of seconds, 1 or more");
                    std::process::exit(2);
                }
            },
            "--install-service" => service = Some(true),
            "--key" => show_key = true,
            "--uninstall-service" => service = Some(false),
            "--keys" => {
                let Some(s) = args.next() else {
                    eprintln!("yagura: --keys needs a script");
                    std::process::exit(2);
                };
                match keyscript::parse(&s) {
                    Ok(steps) => script = Some(steps),
                    Err(e) => {
                        eprintln!("yagura --keys: {e}");
                        std::process::exit(2);
                    }
                }
            }
            other => {
                eprintln!("yagura: unknown argument `{other}`\n\n{USAGE}");
                std::process::exit(2);
            }
        }
    }

    let s = settings::load(None);
    if let Some(install) = service {
        for w in &s.warnings {
            eprintln!("yagura: {w}");
        }
        let done = if install { service::install(s.port) } else { service::uninstall(s.port) };
        if let Err(e) = done {
            eprintln!("yagura: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if snapshot {
        match export::snapshot() {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("yagura --snapshot: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if let Some(path) = &metrics {
        if let Err(e) = export::log_metrics(path, Duration::from_secs_f64(every)) {
            eprintln!("yagura --log-metrics: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if show_key {
        let Some(dir) = &s.dir else {
            eprintln!("yagura --key: this machine has no config folder to keep a key in");
            std::process::exit(1);
        };
        match remote::noise::load_or_make(dir) {
            Ok(keys) => print!("{}", remote::noise::howto(&keys.public, &remote::noise::key_path(dir), &remote::noise::host_name())),
            Err(e) => {
                eprintln!("yagura --key: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if agent {
        if let Some(path) = &log
            && let Err(e) = remote::agent::log_to(path)
        {
            eprintln!("yagura --log: {}: {e}", path.display());
            std::process::exit(1);
        }
        for w in &s.warnings {
            eprintln!("yagura: {w}");
        }
        let direct = s.listen.as_ref().map(|listen| {
            let keys = s.dir.as_deref().ok_or_else(|| "this machine has no config folder to keep a key in".to_string());
            match keys.and_then(|d| remote::noise::load_or_make(d).map_err(|e| e.to_string())) {
                Ok(keys) => remote::agent::Direct { listen: listen.clone(), keys, viewers: s.viewers.clone() },
                Err(e) => {
                    eprintln!("yagura --agent: [agent] listen: {e}");
                    std::process::exit(1);
                }
            }
        });
        let cfg = remote::agent::Config { allow: s.allow.clone(), interval: s.interval, direct };
        if let Err(e) = remote::agent::run(s.port, cfg) {
            eprintln!("yagura --agent: 127.0.0.1:{}: {e}", s.port);
            std::process::exit(1);
        }
        return Ok(());
    }
    i18n::set_language(&s.language);
    let (km, km_warn) = Keymap::load(s.keymap.as_deref());
    let (state_dir, state_note) = persist::dir(s.dir.as_deref());
    let (layouts, stacks, layout_warn) = persist::load(state_dir.as_deref());
    let (wgpu_options, gpu_warn) = gpu_backend::options(s.backend.as_deref());
    let warnings: Vec<String> = s.warnings.iter().chain(&km_warn).chain(&layout_warn).cloned().chain(gpu_warn).chain(state_note).collect();
    for w in &warnings {
        eprintln!("yagura: {w}");
    }
    let saver = persist::Saver::new(state_dir);
    let interval = s.interval;
    let hosts = s.hosts.clone();
    let done = std::env::var_os("YAGURA_KEYS_DONE").filter(|v| !v.is_empty()).map(PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("yagura")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([480.0, 320.0])
            // The title bar, Alt+Tab, the taskbar and the Dock; build.rs bakes
            // the same drawing into yagura.exe for Explorer.
            .with_icon(std::sync::Arc::new(egui::IconData { rgba: icon::rgba(ICON_PX), width: ICON_PX, height: ICON_PX })),
        wgpu_options,
        ..Default::default()
    };
    eframe::run_native(
        "yagura",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(theme::visuals());
            fonts::load_in_background(cc.egui_ctx.clone());
            let (rx, ctl) = collect::spawn(Some(cc.egui_ctx.clone()), Duration::from_secs_f32(interval));
            let mut app = App::new(km, rx, ctl, layouts, saver, interval);
            app.stacks = stacks;
            app.wake = Some(cc.egui_ctx.clone());
            app.prefs = prefs::Prefs::load(settings::base_dir(), s);
            app.prefs.watch(&cc.egui_ctx);
            app.prefs.apply_theme(&cc.egui_ctx);
            // The scale is common.toml's, stepped by the keymap's scale keys,
            // so egui's own Ctrl+= / Ctrl+- stay out of it.
            cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
            cc.egui_ctx.set_zoom_factor(app.prefs.common.scale_or_one());
            if !hosts.is_empty() {
                let peers = hosts.iter().map(remote::Peer::new).collect();
                let fleet = remote::client::spawn(&hosts, app.prefs.settings.dir.clone(), Some(cc.egui_ctx.clone()));
                app.set_fleet(peers, fleet);
            }
            // One line in the status bar; the whole list went to stderr.
            if let Some(first) = warnings.first() {
                let more = if warnings.len() > 1 { format!(" (+{})", warnings.len() - 1) } else { String::new() };
                app.toast(format!("{first}{more}"));
            }
            Ok(Box::new(Yagura { app, script: script.map(|steps| Runner::new(steps, done)), todo: Vec::new() }))
        }),
    )
}
