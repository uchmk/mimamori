//! A system font behind egui's own, for what that lacks: Japanese in the
//! first place (the `ja` table), and the CJK in process names, service
//! descriptions and volume labels whatever the language. The candidates are
//! tsumugi's (`crates/tsumugi/src/fonts.rs`): the first that reads is used.

use eframe::egui;

#[cfg(windows)]
const CANDIDATES: &[&str] = &[
    r"C:\Windows\Fonts\BIZ-UDGothicR.ttc",
    r"C:\Windows\Fonts\msgothic.ttc",
    r"C:\Windows\Fonts\YuGothM.ttc",
    r"C:\Windows\Fonts\meiryo.ttc",
];
#[cfg(target_os = "macos")]
const CANDIDATES: &[&str] = &["/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc", "/System/Library/Fonts/Hiragino Sans GB.ttc"];
#[cfg(not(any(windows, target_os = "macos")))]
const CANDIDATES: &[&str] = &[
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
    "/usr/share/fonts/truetype/fonts-japanese-gothic.ttf",
];

/// Read the first candidate on a thread (the file is tens of megabytes) and
/// put it behind the monospace and proportional faces. Until it is in, a
/// CJK character draws as a box for a frame or two.
pub fn load_in_background(ctx: egui::Context) {
    let _ = std::thread::Builder::new().name("fonts".into()).spawn(move || {
        let Some((path, bytes)) = CANDIDATES.iter().find_map(|p| std::fs::read(p).ok().map(|b| (*p, b))) else { return };
        let mut fonts = egui::FontDefinitions::default();
        let name = format!("system:{path}");
        fonts.font_data.insert(name.clone(), egui::FontData::from_owned(bytes).into());
        for family in [egui::FontFamily::Monospace, egui::FontFamily::Proportional] {
            fonts.families.entry(family).or_default().push(name.clone());
        }
        ctx.set_fonts(fonts);
        ctx.request_repaint();
    });
}
