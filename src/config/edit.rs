//! Writing the settings screen's changes into yagura's config.toml, one
//! key at a time so a person's comments and order stay. common.toml's part
//! (the language, the theme, the clock) is `ito-common`'s.

use std::path::Path;

use ito_common::{quote, quote_list, read_or_empty, set_key, set_tables, write_atomic};

use super::settings::Host;

/// One change to config.toml.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    Interval(f32),
    /// `auto`, `gl`, … (`settings::BACKENDS`).
    Backend(String),
    /// `[agent] allow`.
    Allow(Vec<String>),
    /// `[agent] port`.
    Port(u16),
    /// `check_updates`.
    CheckUpdates(bool),
    /// Every `[[host]]`, in place of the ones there.
    Hosts(Vec<Host>),
    /// `[notify]`'s keys and their values as written, in one go.
    Notify(Vec<(&'static str, String)>),
}

impl Edit {
    /// config.toml's `text` with the change made.
    pub fn apply(&self, text: &str) -> Result<String, String> {
        match self {
            Self::Interval(s) => set_key(text, None, "interval", &format!("{s:?}")),
            Self::Backend(b) => set_key(text, None, "backend", &quote(b)),
            Self::Allow(list) => set_key(text, Some("agent"), "allow", &quote_list(list)),
            Self::Port(p) => set_key(text, Some("agent"), "port", &p.to_string()),
            Self::CheckUpdates(on) => set_key(text, None, "check_updates", &on.to_string()),
            Self::Hosts(hosts) => {
                let blocks: Vec<Vec<(&str, String)>> = hosts.iter().map(host_block).collect();
                set_tables(text, "host", &blocks)
            }
            Self::Notify(keys) => {
                let mut out = text.to_owned();
                for (key, value) in keys {
                    out = set_key(&out, Some("notify"), key, value)?;
                }
                Ok(out)
            }
        }
    }
}

/// One `[[host]]`'s keys; the screen edits the name and the address, the
/// key and the pace go back as they were read.
fn host_block(h: &Host) -> Vec<(&'static str, String)> {
    let mut out = vec![("name", quote(&h.name)), ("addr", quote(&h.addr))];
    if let Some(k) = &h.key {
        out.push(("key", quote(&crate::remote::noise::encode(k))));
    }
    if let Some(every) = h.every {
        out.push(("every", format!("{every:?}")));
    }
    out
}

/// yagura's config.toml for the `uchmk` folder `base`.
pub fn config_path(base: &Path) -> std::path::PathBuf {
    super::settings::own_dir(base).join("config.toml")
}

/// Make the change in the file at once. Call it off the UI thread.
pub fn save(base: &Path, edit: &Edit) -> Result<(), String> {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let path = config_path(base);
    read_or_empty(&path).and_then(|text| edit.apply(&text)).and_then(|next| write_atomic(&path, next))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings;
    use crate::util::test_dir;

    fn host(name: &str, addr: &str) -> Host {
        Host { name: name.into(), addr: addr.into(), ..Host::default() }
    }

    #[test]
    fn a_change_keeps_what_is_around_it() {
        let text = "# sampled every\ninterval = 1.0 # seconds\n\n[agent]\nport = 7313\n";
        let out = Edit::Interval(2.0).apply(text).unwrap();
        assert_eq!(out, "# sampled every\ninterval = 2.0 # seconds\n\n[agent]\nport = 7313\n");
        let out = Edit::Allow(vec!["kill".into()]).apply(&out).unwrap();
        assert!(out.contains("[agent]\nport = 7313\nallow = [\"kill\"]\n"), "{out}");
        let out = Edit::Port(7400).apply(&out).unwrap();
        assert!(out.contains("port = 7400"), "{out}");
        let out = Edit::CheckUpdates(false).apply(&out).unwrap();
        assert!(out.contains("check_updates = false"), "{out}");
        assert!(Edit::Interval(2.0).apply("interval = = 1").is_err());
    }

    #[test]
    fn the_hosts_are_put_in_place_of_the_old_ones() {
        let text = "interval = 1.0\n\n[[host]]\nname = \"old\"\naddr = \"x\"\n\n[agent]\nport = 7313\n";
        let out = Edit::Hosts(vec![host("nas", "localhost:7314"), host("pi \"2\"", "pi.ts.net")]).apply(text).unwrap();
        assert!(!out.contains("old"), "{out}");
        assert!(out.contains("[agent]\nport = 7313"), "{out}");
        let t: toml::Table = out.parse().unwrap();
        let hosts = t["host"].as_array().unwrap();
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[1]["name"].as_str(), Some("pi \"2\""));
        // None left is no [[host]] at all.
        let out = Edit::Hosts(Vec::new()).apply(&out).unwrap();
        assert!(!out.contains("host"), "{out}");
    }

    #[test]
    fn adding_a_host_keeps_the_comments_and_the_agent_where_they_were() {
        let text = "# my machines\n# (the first is the NAS)\n\n[[host]]\nname = \"nas\" # the first host\naddr = \"localhost:7314\"\n\n[agent]\nport = 7323\n";
        let out = Edit::Hosts(vec![host("nas", "localhost:7314"), host("pi", "pi.ts.net")]).apply(text).unwrap();
        assert!(out.starts_with("# my machines\n# (the first is the NAS)\n\n[[host]]\nname = \"nas\" # the first host\n"), "{out}");
        let (pi, agent) = (out.find("name = \"pi\"").unwrap(), out.find("[agent]").unwrap());
        assert!(pi < agent, "the new host lands after the last one, [agent] stays below: {out}");
        assert!(out.ends_with("[agent]\nport = 7323\n"), "{out}");
    }

    #[test]
    fn saved_changes_read_back() {
        let d = test_dir("edit-save");
        save(&d, &Edit::Interval(5.0)).unwrap();
        save(&d, &Edit::Backend("gl".into())).unwrap();
        let keyed = Host { key: Some([7; 32]), every: Some(5.0), ..host("far", "far.example") };
        save(&d, &Edit::Hosts(vec![host("nas", "localhost:7314"), keyed.clone()])).unwrap();
        save(&d, &Edit::Allow(vec!["kill".into(), "service".into()])).unwrap();
        let s = settings::load(Some(&d));
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
        assert_eq!(s.interval, 5.0);
        assert_eq!(s.backend.as_deref(), Some("gl"));
        assert_eq!(s.hosts, [host("nas", "localhost:7314"), keyed], "the key and the pace go back");
        assert_eq!(s.allow, ["kill", "service"]);
    }
}
