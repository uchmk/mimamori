//! Give `mimamori.exe` its icon and version information.
//!
//! This is the icon Explorer draws on the file and a pinned taskbar entry
//! uses, and the name and version Task Manager and the file's Details show;
//! the window's own icon is set at startup from the same drawing
//! (`src/icon.rs`). The `.res` is written in Rust (`src/winres.rs`) and handed
//! to the linker, so no resource compiler is needed and a build for Windows
//! from Linux or macOS gets it too.
//!
//! Only for the MSVC targets: their linkers (`link.exe`, `lld-link`) take a
//! `.res` as it is; the GNU one would want it turned into an object first.

#[path = "src/icon.rs"]
mod icon;
#[path = "src/winres.rs"]
mod winres;

use std::path::PathBuf;

/// The sizes Explorer and the taskbar pick from, at 100 % to 200 % scaling.
const SIDES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=src/winres.rs");

    let var = |k: &str| std::env::var(k).unwrap_or_default();
    if var("CARGO_CFG_TARGET_OS") != "windows" || var("CARGO_CFG_TARGET_ENV") != "msvc" {
        return;
    }
    let icons: Vec<(u32, Vec<u8>)> = SIDES.iter().map(|&side| (side, png(side))).collect();
    let num = |k: &str| var(k).parse().unwrap_or(0);
    let text = var("CARGO_PKG_VERSION");
    let comments = var("CARGO_PKG_DESCRIPTION");
    let info = winres::Info {
        version: [num("CARGO_PKG_VERSION_MAJOR"), num("CARGO_PKG_VERSION_MINOR"), num("CARGO_PKG_VERSION_PATCH")],
        text: &text,
        name: "mimamori",
        comments: &comments,
    };
    let path = PathBuf::from(var("OUT_DIR")).join("mimamori.res");
    std::fs::write(&path, winres::res(&icons, &info)).expect("writing mimamori.res");
    // Every linked target, the tests too: one of them reads it back.
    println!("cargo:rustc-link-arg={}", path.display());
}

/// The icon `side` pixels square as PNG.
fn png(side: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, side, side);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().expect("PNG header");
    w.write_image_data(&icon::rgba(side)).expect("PNG data");
    w.finish().expect("PNG end");
    out
}
