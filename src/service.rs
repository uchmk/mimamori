//! `--install-service` and `--uninstall-service`: the fleet agent started at
//! logon, the way each OS starts a user's own programs. Linux gets a systemd
//! user unit, macOS a launchd LaunchAgent, Windows a value under the user's
//! `Run` key (a logon task in the Task Scheduler needs an administrator).
//! None of them needs more than the user's own rights.
//!
//! What goes into each file is built by a pure function, so every OS's is
//! tested on every OS; the rest writes the file and runs the OS's tool,
//! saying what it did.

#[cfg(not(windows))]
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The service's name: the unit, the `Run` value.
#[cfg(any(target_os = "linux", windows, test))]
const NAME: &str = "yagura-agent";
/// The launchd job's label.
#[cfg(any(target_os = "macos", test))]
const LABEL: &str = "io.github.uchmk.yagura-agent";
/// Where the settings and the state are, when moved: the service is started
/// without the shell that set them.
const CARRIED: [&str; 3] = ["UCHMK_CONFIG_DIR", "YAGURA_CONFIG_HOME", "YAGURA_STATE_HOME"];
/// The user's `Run` key.
#[cfg(any(windows, test))]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

/// Start the agent at logon from now on, and now. `port` is the agent's, to
/// see whether one is already running.
pub fn install(port: u16) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find this program: {e}"))?;
    let exe = exe.to_string_lossy().into_owned();
    let env: Vec<(String, String)> =
        CARRIED.iter().filter_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()).map(|v| (k.to_string(), v))).collect();
    let log = ito_common::state_dir("yagura").map(|d| d.join("agent.log"));
    imp::install(&exe, &env, log.as_deref(), port)
}

/// Stop the agent started at logon and forget it. Nothing installed is fine.
pub fn uninstall(port: u16) -> Result<(), String> {
    if !imp::uninstall(port)? {
        crate::out("yagura: the agent was not installed as a service\n");
    }
    Ok(())
}

/// Run `prog args` and wait, saying what was run; its stderr's last line
/// when it fails.
fn run(prog: &str, args: &[String]) -> Result<(), String> {
    let shown = format!("{prog} {}", args.join(" "));
    let out = Command::new(prog).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).output();
    match out {
        Ok(o) if o.status.success() => {
            crate::out(&format!("ran {shown}\n"));
            Ok(())
        }
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr);
            let why = err
                .lines()
                .map(str::trim)
                .rfind(|l| !l.is_empty())
                .map_or_else(|| format!("exit status {}", o.status.code().unwrap_or(-1)), str::to_string);
            Err(format!("{shown}: {why}"))
        }
        Err(e) => Err(format!("{shown}: {e}")),
    }
}

/// Write `body` to `path`, making its folder.
#[cfg(not(windows))]
fn write(path: &Path, body: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, body).map_err(|e| format!("{}: {e}", path.display()))?;
    crate::out(&format!("wrote {}\n", path.display()));
    Ok(())
}

#[cfg(not(windows))]
fn remove(path: &Path) -> Result<(), String> {
    std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
    crate::out(&format!("removed {}\n", path.display()));
    Ok(())
}

#[cfg(not(windows))]
fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from).ok_or_else(|| "HOME is not set".to_string())
}

/// `s` as one word of a systemd unit: quoted, with its own `%` specifiers
/// and escapes made literal.
#[cfg(any(target_os = "linux", test))]
fn sd_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%"))
}

/// The systemd user unit running `exe --agent` with `env`, started again if
/// it fails (not when it is stopped).
#[cfg(any(target_os = "linux", test))]
pub fn systemd_unit(exe: &str, env: &[(String, String)]) -> String {
    let mut s = String::from("[Unit]\nDescription=yagura fleet agent\nAfter=network.target\n\n[Service]\n");
    s += &format!("ExecStart={} --agent\n", sd_quote(exe));
    for (k, v) in env {
        s += &format!("Environment={}\n", sd_quote(&format!("{k}={v}")));
    }
    s += "Restart=on-failure\nRestartSec=5\n\n[Install]\nWantedBy=default.target\n";
    s
}

/// The LaunchAgent running `exe --agent` with `env` at logon, started again
/// if it fails, saying what it says to `log`.
#[cfg(any(target_os = "macos", test))]
pub fn launchd_plist(exe: &str, env: &[(String, String)], log: Option<&str>) -> Vec<u8> {
    use plist::{Dictionary, Value};

    let mut d = Dictionary::new();
    d.insert("Label".into(), LABEL.into());
    d.insert("ProgramArguments".into(), Value::Array(vec![exe.into(), "--agent".into()]));
    d.insert("RunAtLoad".into(), true.into());
    let mut keep = Dictionary::new();
    keep.insert("SuccessfulExit".into(), false.into());
    d.insert("KeepAlive".into(), Value::Dictionary(keep));
    if !env.is_empty() {
        d.insert("EnvironmentVariables".into(), Value::Dictionary(env.iter().map(|(k, v)| (k.clone(), Value::from(v.as_str()))).collect()));
    }
    if let Some(log) = log {
        d.insert("StandardErrorPath".into(), log.into());
    }
    let mut out = Vec::new();
    Value::Dictionary(d).to_writer_xml(&mut out).expect("a plist in memory");
    out
}

/// The command line the `Run` value holds.
#[cfg(any(windows, test))]
pub fn run_line(exe: &str, log: Option<&str>) -> String {
    match log {
        Some(log) => format!("\"{exe}\" --agent --log \"{log}\""),
        None => format!("\"{exe}\" --agent"),
    }
}

/// `reg` arguments that set the `Run` value to `line`.
#[cfg(any(windows, test))]
pub fn reg_add(line: &str) -> Vec<String> {
    ["add", RUN_KEY, "/v", NAME, "/t", "REG_SZ", "/d", line, "/f"].map(String::from).to_vec()
}

/// `reg` arguments that remove the `Run` value.
#[cfg(any(windows, test))]
pub fn reg_delete() -> Vec<String> {
    ["delete", RUN_KEY, "/v", NAME, "/f"].map(String::from).to_vec()
}

#[cfg(target_os = "linux")]
mod imp {
    use std::path::{Path, PathBuf};

    use super::{NAME, home, remove, run, systemd_unit, write};

    fn unit() -> Result<PathBuf, String> {
        let base = match std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()) {
            Some(d) => PathBuf::from(d),
            None => home()?.join(".config"),
        };
        Ok(base.join("systemd").join("user").join(format!("{NAME}.service")))
    }

    fn systemctl(args: &[&str]) -> Result<(), String> {
        let args: Vec<String> = std::iter::once("--user").chain(args.iter().copied()).map(String::from).collect();
        run("systemctl", &args)
    }

    pub fn install(exe: &str, env: &[(String, String)], _log: Option<&Path>, _port: u16) -> Result<(), String> {
        write(&unit()?, systemd_unit(exe, env).as_bytes())?;
        systemctl(&["daemon-reload"])?;
        systemctl(&["enable", "--now", &format!("{NAME}.service")])?;
        crate::out(&format!("the agent starts at logon; `journalctl --user -u {NAME}` shows what it says\n"));
        Ok(())
    }

    pub fn uninstall(_port: u16) -> Result<bool, String> {
        let path = unit()?;
        if !path.exists() {
            return Ok(false);
        }
        let stopped = systemctl(&["disable", "--now", &format!("{NAME}.service")]);
        remove(&path)?;
        let _ = systemctl(&["daemon-reload"]);
        stopped.map(|()| true)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use super::{LABEL, home, launchd_plist, remove, run, write};
    use crate::collect::svc::mac::uid;

    fn plist() -> Result<PathBuf, String> {
        Ok(home()?.join("Library").join("LaunchAgents").join(format!("{LABEL}.plist")))
    }

    /// Unload the job if it is loaded; not loaded is fine.
    fn bootout() -> bool {
        let target = format!("gui/{}/{LABEL}", uid());
        Command::new("launchctl")
            .args(["bootout", &target])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    pub fn install(exe: &str, env: &[(String, String)], log: Option<&Path>, _port: u16) -> Result<(), String> {
        if let Some(dir) = log.and_then(Path::parent) {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let path = plist()?;
        let log = log.map(|l| l.to_string_lossy().into_owned());
        write(&path, &launchd_plist(exe, env, log.as_deref()))?;
        if bootout() {
            crate::out("unloaded the agent loaded before\n");
        }
        run("launchctl", &["bootstrap".to_string(), format!("gui/{}", uid()), path.to_string_lossy().into_owned()])?;
        if let Some(log) = log {
            crate::out(&format!("the agent starts at logon; it writes to {log}\n"));
        }
        Ok(())
    }

    pub fn uninstall(_port: u16) -> Result<bool, String> {
        let path = plist()?;
        if !path.exists() {
            return Ok(false);
        }
        if bootout() {
            crate::out("stopped the agent\n");
        }
        remove(&path)?;
        Ok(true)
    }
}

#[cfg(windows)]
mod imp {
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::{Command, Stdio};

    use super::{RUN_KEY, reg_add, reg_delete, run, run_line};
    use crate::kill;
    use crate::remote::client::{self, Why};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    pub fn install(exe: &str, env: &[(String, String)], log: Option<&Path>, port: u16) -> Result<(), String> {
        if !env.is_empty() {
            let names: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
            crate::err(&format!("yagura: {} will not reach the agent started at logon; set them for the user instead\n", names.join(", ")));
        }
        let log = log.map(|l| l.to_string_lossy().into_owned());
        run("reg", &reg_add(&run_line(exe, log.as_deref())))?;
        // The Run key starts it at the next logon; this one starts it now.
        match client::probe(port) {
            Ok((hello, _)) => crate::out(&format!("an agent is already running (process {})\n", hello.pid)),
            Err(Why::Refused) => {
                let mut cmd = Command::new(exe);
                cmd.arg("--agent");
                if let Some(log) = &log {
                    cmd.args(["--log", log]);
                }
                let child = cmd
                    .creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|e| format!("{exe} --agent: {e}"))?;
                crate::out(&format!("started the agent (process {})\n", child.id()));
            }
            Err(e) => crate::err(&format!("yagura: 127.0.0.1:{port}: {}\n", crate::ui::fleet::why_text(&e))),
        }
        if let Some(log) = log {
            crate::out(&format!("the agent starts at logon; it writes to {log}\n"));
        }
        Ok(())
    }

    pub fn uninstall(port: u16) -> Result<bool, String> {
        let there = Command::new("reg")
            .args(["query", RUN_KEY, "/v", super::NAME])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !there {
            return Ok(false);
        }
        run("reg", &reg_delete())?;
        if let Ok((hello, _)) = client::probe(port) {
            let t = kill::Target { pid: hello.pid, name: "yagura".into(), start: 0 };
            match kill::end(&t, kill::GRACE) {
                kill::Outcome::Failed(e) => return Err(format!("process {}: {e}", hello.pid)),
                _ => crate::out(&format!("stopped the agent (process {})\n", hello.pid)),
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_systemd_unit_quotes_the_program_and_carries_the_settings() {
        let env = vec![("UCHMK_CONFIG_DIR".to_string(), "/home/a b/cfg".to_string())];
        let u = systemd_unit("/opt/my apps/yagura", &env);
        assert!(u.contains("\nExecStart=\"/opt/my apps/yagura\" --agent\n"), "{u}");
        assert!(u.contains("\nEnvironment=\"UCHMK_CONFIG_DIR=/home/a b/cfg\"\n"), "{u}");
        assert!(u.contains("\nRestart=on-failure\n") && u.ends_with("[Install]\nWantedBy=default.target\n"), "{u}");
        // systemd's own specifiers and escapes stay literal.
        assert_eq!(sd_quote(r#"C:\50% "x""#), r#""C:\\50%% \"x\"""#);
        assert!(!systemd_unit("/y", &[]).contains("Environment="));
    }

    #[test]
    fn the_launch_agent_runs_the_agent_at_load_and_after_a_failure() {
        let env = vec![("YAGURA_CONFIG_HOME".to_string(), "/tmp/c".to_string())];
        let bytes = launchd_plist("/Applications/yagura", &env, Some("/tmp/agent.log"));
        let v = plist::Value::from_reader(std::io::Cursor::new(bytes)).unwrap();
        let d = v.as_dictionary().unwrap();
        assert_eq!(d.get("Label").and_then(|v| v.as_string()), Some(LABEL));
        let args: Vec<&str> = d.get("ProgramArguments").and_then(|v| v.as_array()).unwrap().iter().filter_map(|v| v.as_string()).collect();
        assert_eq!(args, ["/Applications/yagura", "--agent"]);
        assert_eq!(d.get("RunAtLoad").and_then(|v| v.as_boolean()), Some(true));
        let keep = d.get("KeepAlive").and_then(|v| v.as_dictionary()).unwrap();
        assert_eq!(keep.get("SuccessfulExit").and_then(|v| v.as_boolean()), Some(false));
        let envs = d.get("EnvironmentVariables").and_then(|v| v.as_dictionary()).unwrap();
        assert_eq!(envs.get("YAGURA_CONFIG_HOME").and_then(|v| v.as_string()), Some("/tmp/c"));
        assert_eq!(d.get("StandardErrorPath").and_then(|v| v.as_string()), Some("/tmp/agent.log"));
        let bare = plist::Value::from_reader(std::io::Cursor::new(launchd_plist("/y", &[], None))).unwrap();
        assert!(bare.as_dictionary().unwrap().get("EnvironmentVariables").is_none());
    }

    #[test]
    fn the_run_value_quotes_the_program_and_the_log() {
        let line = run_line(r"C:\Program Files\yagura\yagura.exe", Some(r"C:\Users\a\AppData\Local\uchmk\yagura\agent.log"));
        assert_eq!(line, r#""C:\Program Files\yagura\yagura.exe" --agent --log "C:\Users\a\AppData\Local\uchmk\yagura\agent.log""#);
        assert_eq!(run_line(r"C:\y.exe", None), r#""C:\y.exe" --agent"#);
        let add = reg_add(&line);
        assert_eq!(add[..4], ["add", RUN_KEY, "/v", NAME]);
        assert_eq!(add[7], line);
        assert_eq!(add.last().map(String::as_str), Some("/f"));
        assert_eq!(reg_delete(), ["delete", RUN_KEY, "/v", NAME, "/f"]);
    }
}
