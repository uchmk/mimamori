//! launchd's jobs through `launchctl list`: a PID (or `-`), the last exit
//! status and the label, one job a line. As a user it lists that user's
//! jobs; the system's need root.

use super::{Service, State};

#[cfg(target_os = "macos")]
pub fn read() -> super::Services {
    use std::process::{Command, Stdio};

    use super::{Note, Services};

    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    // SAFETY: geteuid takes nothing and cannot fail.
    let root = unsafe { geteuid() } == 0;
    let note = (!root).then_some(Note::MacUser);
    let out = Command::new("launchctl").arg("list").stdin(Stdio::null()).stderr(Stdio::null()).output();
    let list = match out {
        Ok(o) if o.status.success() => parse_list(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    };
    Services { list, source: "launchctl", note }
}

/// The rows under the `PID Status Label` header. A job with a PID runs; one
/// without is loaded and waiting, failed when its last exit was not 0 (a
/// negative status is the signal that ended it).
pub fn parse_list(s: &str) -> Vec<Service> {
    s.lines()
        .filter_map(|l| {
            let mut it = l.split('\t');
            let (pid, status, label) = (it.next()?.trim(), it.next()?.trim(), it.next()?.trim());
            if label.is_empty() || pid == "PID" {
                return None;
            }
            let pid = pid.parse::<u32>().ok();
            let exit = status.parse::<i64>().ok();
            let state = match (pid, exit) {
                (Some(_), _) => State::Running,
                (None, Some(0) | None) => State::Stopped,
                (None, Some(_)) => State::Failed,
            };
            Some(Service { name: label.to_string(), state, pid, exit, ..Service::default() })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launchctl_rows_become_jobs() {
        let v = parse_list(
            "PID\tStatus\tLabel\n\
             412\t0\tcom.apple.Finder\n\
             -\t0\tcom.apple.SafariHistoryServiceAgent\n\
             -\t-9\tcom.example.killed\n\
             -\t78\tcom.example.broken\n\
             not a row\n",
        );
        let got: Vec<(&str, State, Option<u32>, Option<i64>)> = v.iter().map(|s| (s.name.as_str(), s.state, s.pid, s.exit)).collect();
        assert_eq!(
            got,
            [
                ("com.apple.Finder", State::Running, Some(412), Some(0)),
                ("com.apple.SafariHistoryServiceAgent", State::Stopped, None, Some(0)),
                ("com.example.killed", State::Failed, None, Some(-9)),
                ("com.example.broken", State::Failed, None, Some(78)),
            ]
        );
    }
}
