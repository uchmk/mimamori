//! mimamori: a keyboard-driven system monitor, the sibling of filer and
//! tsumugi. The window and the input; the state is in `app.rs`, the
//! drawing in `ui/`, the sampling in `collect/`.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod collect;
mod config;
mod fonts;
mod gpu_backend;
mod i18n;
mod keyscript;
mod kill;
mod model;
mod persist;
mod screen;
mod svcctl;
mod tree;
mod ui;
mod util;

use std::path::PathBuf;
use std::time::Duration;

use app::App;
use config::keys::{Key, from_egui};
use config::{keymap::Keymap, settings, theme};
use keyscript::{Do, Runner};

const USAGE: &str = "\
mimamori — a keyboard-driven system monitor

usage: mimamori [--keys SCRIPT]

  --keys SCRIPT  press these keys once the window is up, as filer's --keys
                 (`<Wait:ms>`, `<Shot:name>`, `<State:name>`, `<Click:x,y>`,
                 `<Drag:x,y-x,y>`, `<Quit>`); MIMAMORI_KEYS_DONE names the
                 file the final state is written to
  --version      print the version
  --help         print this

Settings: <config dir>/uchmk/mimamori/config.toml and keymap.toml;
the language can also be set for every uchmk app in <config dir>/uchmk/common.toml.";

struct Mimamori {
    app: App,
    script: Option<Runner>,
    /// What the script asked for in `raw_input_hook`, done in the frame.
    todo: Vec<Do>,
}

impl Mimamori {
    fn write_state(&self, path: PathBuf, tail: &str) {
        let text = format!("{}{tail}", self.app.report());
        if let Err(e) = std::fs::write(&path, text) {
            eprintln!("mimamori --keys: {}: {e}", path.display());
        }
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

impl eframe::App for Mimamori {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some(script) = &mut self.script
            && let Some(d) = script.input(ctx, raw)
        {
            self.todo.push(d);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.keys(&ctx);
        self.script(&ctx);
        self.app.pump();
        let out = ui::frame(ui, &self.app);
        self.app.apply_out(out, &ctx);
        if self.app.quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // One more frame to take the toast away; nothing else needs a frame
        // until the sampler sends one.
        if let Some(left) = self.app.toast_left() {
            ctx.request_repaint_after(left);
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        theme::BG.to_normalized_gamma_f32()
    }
}

fn main() -> eframe::Result {
    let mut args = std::env::args().skip(1);
    let mut script = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--version" | "-V" => {
                println!("mimamori {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            "--keys" => {
                let Some(s) = args.next() else {
                    eprintln!("mimamori: --keys needs a script");
                    std::process::exit(2);
                };
                match keyscript::parse(&s) {
                    Ok(steps) => script = Some(steps),
                    Err(e) => {
                        eprintln!("mimamori --keys: {e}");
                        std::process::exit(2);
                    }
                }
            }
            other => {
                eprintln!("mimamori: unknown argument `{other}`\n\n{USAGE}");
                std::process::exit(2);
            }
        }
    }

    let s = settings::load(None);
    i18n::set_language(&s.language);
    let (km, km_warn) = Keymap::load(s.keymap.as_deref());
    let (layouts, layout_warn) = persist::load(s.dir.as_deref());
    let (wgpu_options, gpu_warn) = gpu_backend::options(s.backend.as_deref());
    let warnings: Vec<String> = s.warnings.iter().chain(&km_warn).chain(&layout_warn).cloned().chain(gpu_warn).collect();
    for w in &warnings {
        eprintln!("mimamori: {w}");
    }
    let saver = persist::Saver::new(s.dir.clone());
    let interval = s.interval;
    let done = std::env::var_os("MIMAMORI_KEYS_DONE").filter(|v| !v.is_empty()).map(PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("mimamori")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([480.0, 320.0]),
        wgpu_options,
        ..Default::default()
    };
    eframe::run_native(
        "mimamori",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(theme::visuals());
            fonts::load_in_background(cc.egui_ctx.clone());
            let (rx, ctl) = collect::spawn(cc.egui_ctx.clone(), Duration::from_secs_f32(interval));
            let mut app = App::new(km, rx, ctl, layouts, saver, interval);
            app.wake = Some(cc.egui_ctx.clone());
            // One line in the status bar; the whole list went to stderr.
            if let Some(first) = warnings.first() {
                let more = if warnings.len() > 1 { format!(" (+{})", warnings.len() - 1) } else { String::new() };
                app.toast(format!("{first}{more}"));
            }
            Ok(Box::new(Mimamori { app, script: script.map(|steps| Runner::new(steps, done)), todo: Vec::new() }))
        }),
    )
}
