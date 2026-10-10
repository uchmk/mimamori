//! A system font behind egui's own, for what that lacks: Japanese in the
//! first place (the `ja` table), and the CJK in process names, service
//! descriptions and volume labels whatever the language. The candidates are
//! every uchmk app's (`ito_common::fonts::JAPANESE`): the first that reads is
//! used.

use eframe::egui;

/// Read the first candidate on a thread (the file is tens of megabytes) and
/// put it behind the monospace and proportional faces. Until it is in, a
/// CJK character draws as a box for a frame or two.
pub fn load_in_background(ctx: egui::Context) {
    let _ = std::thread::Builder::new().name("fonts".into()).spawn(move || {
        let Some((path, bytes)) = ito_common::fonts::read_first(ito_common::fonts::JAPANESE) else { return };
        let mut fonts = egui::FontDefinitions::default();
        let name = format!("system:{}", path.display());
        fonts.font_data.insert(name.clone(), egui::FontData::from_owned(bytes).into());
        for family in [egui::FontFamily::Monospace, egui::FontFamily::Proportional] {
            fonts.families.entry(family).or_default().push(name.clone());
        }
        ctx.set_fonts(fonts);
        ctx.request_repaint();
    });
}
