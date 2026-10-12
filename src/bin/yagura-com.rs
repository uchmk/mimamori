//! `yagura.com`: the console-subsystem front for `yagura.exe`.
//!
//! `yagura.exe` is a windowed program, so PowerShell does not wait for it at
//! the end of a pipeline: `$v = & yagura env` comes back empty, as does
//! `& yagura --version`, and the text arrives after the prompt (uchmk/yagura#15).
//! The release zip ships this binary beside `yagura.exe` as `yagura.com`.
//! `PATHEXT` lists `.COM` before `.EXE`, so typing `yagura` finds this one
//! first. It is the shape of Visual Studio's `devenv.com` and `devenv.exe`.
//!
//! For the commands that answer in text, it runs `yagura.exe` with the same
//! arguments and the same standard handles, waits, and passes on the exit
//! code. Being a console program, it is what the shell waits for. Anything
//! else opens the window: it starts `yagura.exe` and returns as soon as the
//! window is taking input, much as typing `yagura.exe` always did. It waits
//! that long, not less, so that a command line `yagura.exe` refuses before
//! opening a window (`--keys "<Tab"`) still prints its one line
//! here and passes on its exit code.
//!
//! The window does not get this console's standard handles. When they are a
//! pipe (`pwsh -File x.ps1 | Tee-Object`), the reader waits until every
//! process holding the pipe has let go, and with the window holding it that
//! was until the window closed (#192). The window writes into pipes of this
//! program's own instead, copied out here until the window is up.
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
    let mut args = args.iter().map(|a| a.to_str().unwrap_or(""));
    while let Some(a) = args.next() {
        if ANSWERS.contains(&a) {
            return true;
        }
        if TAKES_VALUE.contains(&a) {
            args.next();
        }
    }
    false
}

#[cfg(windows)]
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let exe = match std::env::current_exe() {
        Ok(me) => me.with_file_name("yagura.exe"),
        Err(e) => {
            eprintln!("yagura.com: cannot tell where it is: {e}");
            std::process::exit(2);
        }
    };
    if !exe.is_file() {
        eprintln!("yagura.com: no yagura.exe beside it ({})", exe.display());
        std::process::exit(2);
    }
    let mut cmd = std::process::Command::new(&exe);
    cmd.args(&args);
    if answers_in_text(&args) {
        match cmd.status() {
            Ok(status) => std::process::exit(status.code().unwrap_or(1)),
            Err(e) => {
                eprintln!("yagura.com: {}: {e}", exe.display());
                std::process::exit(2);
            }
        }
    }
    keep_own_handles();
    let piped = std::process::Stdio::piped;
    let mut child = match cmd.stdin(std::process::Stdio::null()).stdout(piped()).stderr(piped()).spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("yagura.com: {}: {e}", exe.display());
            std::process::exit(2);
        }
    };
    let copied = [copy_out(child.stdout.take(), std::io::stdout), copy_out(child.stderr.take(), std::io::stderr)];
    let code = until_the_window_is_up(&mut child);
    if let Ok(Some(_)) = child.try_wait() {
        // It refused: let its words reach the console before the exit code.
        // Bounded, because a program it started may still hold the pipe.
        for done in copied {
            let _ = done.recv_timeout(std::time::Duration::from_secs(2));
        }
    }
    std::process::exit(code);
}

/// Makes this program's standard handles stay here. `Command` starts
/// `yagura.exe` with every inheritable handle, not only the three it is given,
/// so the pipe of a `| Tee-Object` would reach the window all the same.
#[cfg(windows)]
fn keep_own_handles() {
    use windows::Win32::Foundation::{HANDLE_FLAG_INHERIT, HANDLE_FLAGS, SetHandleInformation};
    use windows::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};

    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        if let Ok(h) = unsafe { GetStdHandle(which) }
            && !h.is_invalid()
        {
            let _ = unsafe { SetHandleInformation(h, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)) };
        }
    }
}

/// Copies what the window writes to this console, on a thread of its own so
/// that a long refusal cannot fill the pipe and stall `yagura.exe`. The
/// receiver hears once the pipe is closed and everything is copied.
#[cfg(windows)]
fn copy_out<R, W>(from: Option<R>, to: fn() -> W) -> std::sync::mpsc::Receiver<()>
where
    R: std::io::Read + Send + 'static,
    W: std::io::Write + 'static,
{
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        if let Some(mut from) = from {
            let mut out = to();
            let _ = std::io::copy(&mut from, &mut out);
            let _ = out.flush();
        }
        let _ = done.send(());
    });
    finished
}

/// Waits for the window to start taking input, then lets it be: the prompt
/// comes back with yagura still open. If `yagura.exe` exits first, it refused
/// the command line, and its exit code is this one's. A refusal comes before
/// any window, within milliseconds of starting; the cap is for a machine so
/// slow that neither has happened yet.
#[cfg(windows)]
fn until_the_window_is_up(child: &mut std::process::Child) -> i32 {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Foundation::WAIT_OBJECT_0;
    use windows::Win32::System::Threading::{WaitForInputIdle, WaitForSingleObject};

    let process = HANDLE(child.as_raw_handle());
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.code().unwrap_or(1),
            Ok(None) => {}
            Err(_) => return 0,
        }
        match unsafe { WaitForInputIdle(process, 50) } {
            // Idle: its message loop is running and waiting for input -- or
            // it has exited, which WaitForInputIdle also answers with 0. CI
            // caught that: `--keys "<Tab"` printed its refusal, yagura.exe
            // exited 2, and this returned 0. So look again, giving an exit
            // that is already under way a moment to land.
            0 => {
                if unsafe { WaitForSingleObject(process, 250) } == WAIT_OBJECT_0 {
                    return match child.try_wait() {
                        Ok(Some(status)) => status.code().unwrap_or(1),
                        _ => 1,
                    };
                }
                return 0;
            }
            // WAIT_TIMEOUT: not yet.
            0x102 => {}
            // WAIT_FAILED: no message queue to wait on, which is a debug
            // build (a console program). Give a refusal its moment, then go.
            _ => {
                std::thread::sleep(std::time::Duration::from_millis(300));
                return match child.try_wait() {
                    Ok(Some(status)) => status.code().unwrap_or(1),
                    _ => 0,
                };
            }
        }
        if start.elapsed() > std::time::Duration::from_secs(10) {
            return 0;
        }
    }
}

#[cfg(not(windows))]
fn main() {
    let _ = answers_in_text;
    eprintln!("yagura-com is the console front for yagura.exe, and only does anything on Windows");
    std::process::exit(2);
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
