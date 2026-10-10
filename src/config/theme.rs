//! The colours, from the theme in force (common.toml's `theme`, shared by
//! every uchmk app; `tsumugi Dark` until one is read). Each is a function,
//! so a theme picked while mimamori runs is drawn from the next frame.

use egui::Color32;
use ito_theme::{Colors, mix};

fn now() -> Colors {
    ito_theme::colors()
}

/// The window's background.
pub fn bg() -> Color32 {
    now().bg
}
/// A panel a step up from the background: the help, a dialog.
pub fn bg_alt() -> Color32 {
    now().panel
}
/// The empty part of a bar.
pub fn bar() -> Color32 {
    bar_of(&now())
}
pub fn line() -> Color32 {
    now().border
}
pub fn fg() -> Color32 {
    now().fg
}
pub fn dim() -> Color32 {
    now().dim
}
pub fn cyan() -> Color32 {
    now().run
}
/// Behind the selected row of the focused column.
pub fn sel() -> Color32 {
    let c = now();
    mix(c.bg, c.blue, 0.3)
}
/// Behind the selected row of a column without the focus.
pub fn sel_unfocused() -> Color32 {
    let c = now();
    mix(c.bg, c.fg, 0.1)
}
pub fn blue() -> Color32 {
    now().blue
}
pub fn green() -> Color32 {
    now().done
}
pub fn yellow() -> Color32 {
    now().wait
}
pub fn red() -> Color32 {
    now().err
}
pub fn magenta() -> Color32 {
    now().magenta
}
/// The clock's figures: the theme's strongest text.
pub fn strong() -> Color32 {
    now().strong()
}

/// The empty part of a bar in `c`.
pub fn bar_of(c: &Colors) -> Color32 {
    mix(c.panel, c.border, 0.3)
}

/// One row of a list.
pub const ROW: f32 = 23.0;

/// The usage ramp: blue when idle, cyan from 30 %, yellow from 70 %, red
/// from 90 %.
pub fn ramp(pct: f32) -> Color32 {
    match pct {
        p if p >= 90.0 => red(),
        p if p >= 70.0 => yellow(),
        p if p >= 30.0 => cyan(),
        _ => blue(),
    }
}

/// egui's widgets in the theme's colours; set again when the theme changes.
pub fn visuals() -> egui::Visuals {
    let c = now();
    let mut v = c.visuals();
    v.selection.bg_fill = sel();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ramp_steps_at_30_70_90() {
        assert_eq!(ramp(0.0), blue());
        assert_eq!(ramp(30.0), cyan());
        assert_eq!(ramp(69.9), cyan());
        assert_eq!(ramp(70.0), yellow());
        assert_eq!(ramp(90.0), red());
    }

    /// The default theme keeps mimamori's colours from before themes: the
    /// bar and the selection within a few steps of the old constants.
    #[test]
    fn tsumugi_dark_is_the_old_look() {
        let dark = ito_theme::builtin()[0].colors;
        assert_eq!(bar_of(&dark), Color32::from_rgb(0x20, 0x23, 0x2a));
        let near = |a: Color32, b: Color32| a.r().abs_diff(b.r()) < 16 && a.g().abs_diff(b.g()) < 16 && a.b().abs_diff(b.b()) < 16;
        assert!(near(mix(dark.bg, dark.blue, 0.3), Color32::from_rgb(0x2f, 0x4a, 0x6b)));
        assert!(near(mix(dark.bg, dark.fg, 0.1), Color32::from_rgb(0x26, 0x2b, 0x34)));
    }
}
