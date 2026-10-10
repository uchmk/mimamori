//! yazi's key notation: `a`, `Z`, `<C-a>`, `<Enter>`, `<F5>`, `<A-S-Up>` …
//!
//! The parsing, printing and egui conversion live in the shared crate
//! `ito-keys` (in uchmk/ito), so the apps spell keys the same
//! way; `P-` is the primary modifier, Cmd on macOS and Ctrl elsewhere.
//! What a key does (the layers in `keymap.rs`, the commands in `cmd.rs`) stays
//! here.

pub use ito_keys::*;
