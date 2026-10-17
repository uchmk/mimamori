//! Write `TESTING-CHECKS.md`: the manual half of TESTING.md as a checklist with
//! a box to tick per row.
//!
//! The generator is ito's `ito-testcheck` (shared with kura and tsumugi); this
//! is yagura's configuration of it. Ticks survive regeneration and rows a test
//! names are left out.
//!
//!     cargo run --example make-testcheck [-- --check | --stats]
//!
//! `--check` writes nothing and exits 1 when the file is out of date (what CI
//! runs). `[~]` is an appearance row a lane judged from a screenshot, not done
//! until the owner looks.

use ito_testcheck::testcheck::{Config, Counts, Lane, run};

const SRC: &str = "TESTING.md";

fn main() {
    run(&Config {
        source: SRC,
        notes: "scripts/testcheck.toml",
        src_dirs: &["src", "tests"],
        dep_crates: &[],
        release_list: None,
        command: "cargo run --example make-testcheck",
        lanes: &[Lane { name: "windows", out: "TESTING-CHECKS.md", marks: &['~'], setup: false, check_release: false, preamble }],
    });
}

fn preamble(c: &Counts) -> String {
    format!(
        "# Manual checklist\n\n\
         Generated from {SRC} by `cargo run --example make-testcheck`. **{SRC} is the source**; \
         ticks (`[x]`) are the only thing to write by hand here, and they survive regeneration.\n\n\
         {} rows to press ({} in {SRC}, {} of them covered by `cargo test`). \
         Progress per section: `cargo run --example make-testcheck -- --stats`. \
         `[~]` does not count as done until the owner looks at the same picture.\n",
        c.manual,
        c.all,
        c.all - c.manual,
    )
}
