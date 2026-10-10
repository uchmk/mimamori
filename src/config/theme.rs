//! The colours: filer's default theme, so the two look like one family.

use egui::Color32;

pub const BG: Color32 = Color32::from_rgb(0x16, 0x18, 0x1d);
pub const BG_ALT: Color32 = Color32::from_rgb(0x1b, 0x1e, 0x24);
pub const BAR: Color32 = Color32::from_rgb(0x20, 0x23, 0x2b);
pub const LINE: Color32 = Color32::from_rgb(0x2c, 0x30, 0x39);
pub const FG: Color32 = Color32::from_rgb(0xc8, 0xcd, 0xd8);
pub const DIM: Color32 = Color32::from_rgb(0x79, 0x80, 0x90);
pub const CYAN: Color32 = Color32::from_rgb(0x6f, 0xd0, 0xd0);
pub const SEL: Color32 = Color32::from_rgb(0x2f, 0x4a, 0x6b);
pub const SEL_UNFOCUSED: Color32 = Color32::from_rgb(0x26, 0x2b, 0x34);
pub const BLUE: Color32 = Color32::from_rgb(0x7a, 0xb8, 0xf5);
pub const GREEN: Color32 = Color32::from_rgb(0x8e, 0xd0, 0x8e);
pub const YELLOW: Color32 = Color32::from_rgb(0xe8, 0xc8, 0x7a);
pub const RED: Color32 = Color32::from_rgb(0xf0, 0x71, 0x78);
pub const MAGENTA: Color32 = Color32::from_rgb(0xc9, 0x9c, 0xf0);

/// One row of a list.
pub const ROW: f32 = 23.0;

/// The usage ramp: blue when idle, cyan from 30 %, yellow from 70 %, red
/// from 90 %.
pub fn ramp(pct: f32) -> Color32 {
    match pct {
        p if p >= 90.0 => RED,
        p if p >= 70.0 => YELLOW,
        p if p >= 30.0 => CYAN,
        _ => BLUE,
    }
}

/// egui's widgets in these colours.
pub fn visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = BG_ALT;
    v.extreme_bg_color = BG;
    v.faint_bg_color = BG_ALT;
    v.override_text_color = Some(FG);
    v.window_stroke = egui::Stroke::new(1.0, LINE);
    v.selection.bg_fill = SEL;
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, LINE);
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, FG);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ramp_steps_at_30_70_90() {
        assert_eq!(ramp(0.0), BLUE);
        assert_eq!(ramp(30.0), CYAN);
        assert_eq!(ramp(69.9), CYAN);
        assert_eq!(ramp(70.0), YELLOW);
        assert_eq!(ramp(90.0), RED);
    }
}
