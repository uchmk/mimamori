//! `yagura.com`: the console-subsystem front for `yagura.exe`.
//!
//! `yagura.exe` is a windowed program, so PowerShell does not wait for it at
//! the end of a pipeline: `$v = & yagura env` comes back empty, as does
//! `& yagura --version`, and the text arrives after the prompt (uchmk/yagura#15).
//! The release zip ships this binary beside `yagura.exe` as `yagura.com`.
//! `PATHEXT` lists `.COM` before `.EXE`, so typing `yagura` finds this one
//! first. It is the shape of Visual Studio's `devenv.com` and `devenv.exe`.
//!
//! The front itself is ito's `ito_console::front` (v0.66.0), shared with
//! kura, tsumugi, minato and michi; what is yagura's is which command lines
//! answer in text. For those it runs `yagura.exe` with the same arguments
//! and the same standard handles, waits, and passes on the exit code.
//! Anything else opens the window: it starts `yagura.exe` and returns as soon
//! as the window is taking input, so that a command line `yagura.exe` refuses
//! before opening a window (`--keys "<Tab"`) still prints its one line here
//! and passes on its exit code. The window does not hold this console's pipe
//! (#192): from a pipe, it writes into a file of its own in the temporary
//! folder, copied out here once it is up.
//!
//! It holds no logic of its own beyond that choice: the report, the help and
//! the hooks all come from `yagura.exe`, so the two cannot disagree.

/// The arguments that make `yagura.exe` print and exit rather than open a
/// window. Kept in step with `parse_cli` in `main.rs`.
/// `--agent` too: it serves on the standard handles' console until stopped.
const ANSWERS: &[&str] = &[
    "env",
    "--env",
    "--version",
    "-V",
    "--help",
    "-h",
    "--agent",
    "--snapshot",
    "--log-metrics",
    "--install-service",
    "--uninstall-service",
    "--key",
];

/// The options that take the next argument as their value, which is then
/// never a command: `--keys env` types three letters.
const TAKES_VALUE: &[&str] = &["--keys", "--log", "--log-metrics", "--every", "--out"];

/// Whether `yagura.exe` will answer this command line in text and exit, read
/// the way `parse_cli` reads it: left to right, skipping option values.
fn answers_in_text(args: &[std::ffi::OsString]) -> bool {
    ito_console::any_of(args, ANSWERS, TAKES_VALUE)
}

fn main() {
    ito_console::front("yagura", answers_in_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(args: &[&str]) -> Vec<std::ffi::OsString> {
        args.iter().map(Into::into).collect()
    }

    /// The text commands are waited for; anything that opens the window is not.
    #[test]
    fn it_waits_only_for_what_answers_in_text() {
        for waits in [
            &["env"][..],
            &["env", "--out", "r.txt"],
            &["--version"],
            &["-V"],
            &["--help"],
            &["--agent", "--log", "a.log"],
            &["--snapshot"],
            &["--log-metrics", "m.csv", "--every", "5"],
            &["--install-service"],
            &["--key"],
            &["--keys", "j", "-V"],
        ] {
            assert!(answers_in_text(&line(waits)), "{waits:?}");
        }
        for opens in [&[][..], &["--keys", "<Tab>"], &["environment"], &["--keys", "env"], &["--keys", "--help"]] {
            assert!(!answers_in_text(&line(opens)), "{opens:?}");
        }
    }
}
