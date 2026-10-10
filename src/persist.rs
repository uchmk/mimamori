//! `state.toml`: what yagura writes for itself, the column widths of each
//! screen and the shares of the panels stacked in a column. Read once at startup; written by a thread of its own whenever a
//! divider settles, so the UI never waits on the disk.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crossbeam_channel::{Sender, unbounded};
use ito_layout::Node;
use serde::{Deserialize, Serialize};

use crate::screen::{Pane, Screen};
use crate::stacks;

#[derive(Default, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    layouts: BTreeMap<String, Node<Pane>>,
    /// `cpu.mid` and so on: the panels of a column, as the mouse left them.
    #[serde(default)]
    stacks: BTreeMap<String, Node<u8>>,
}

/// The panels of each screen's columns, by screen and column.
pub type Stacks = HashMap<(Screen, Pane), Node<u8>>;

fn stack_key(s: Screen, p: Pane) -> String {
    format!("{}.{}", s.name(), p.name())
}

fn stack_of(key: &str) -> Option<(Screen, Pane)> {
    let (s, p) = key.split_once('.')?;
    Some((*Screen::ALL.iter().find(|x| x.name() == s)?, *Pane::ALL.iter().find(|x| x.name() == p)?))
}

/// A layout holding exactly the three columns, each once; anything else
/// (a hand edit, an older file) is not trusted.
fn valid(n: &Node<Pane>) -> bool {
    let mut leaves = n.leaves();
    leaves.sort_by_key(|p| p.index());
    leaves == Pane::ALL
}

/// Every screen's layout (the saved one where it reads, else the default)
/// and the saved stacks.
pub fn load(dir: Option<&Path>) -> (HashMap<Screen, Node<Pane>>, Stacks, Vec<String>) {
    let mut warn = Vec::new();
    let mut saved = State::default();
    if let Some(text) = dir.and_then(|d| std::fs::read_to_string(d.join("state.toml")).ok()) {
        match toml::from_str::<State>(&text) {
            Ok(s) => saved = s,
            Err(e) => warn.push(format!("state.toml: {e}")),
        }
    }
    let mut out = HashMap::new();
    for s in Screen::ALL {
        let node = match saved.layouts.remove(s.name()) {
            Some(n) if valid(&n) => n,
            Some(_) => {
                warn.push(format!("state.toml: the {} layout is not the three columns; using the default", s.name()));
                s.default_layout()
            }
            None => s.default_layout(),
        };
        out.insert(s, node);
    }
    let mut stacks = Stacks::new();
    for (key, node) in saved.stacks {
        match stack_of(&key) {
            Some(at) if stacks::valid(&node) => {
                stacks.insert(at, node);
            }
            _ => warn.push(format!("state.toml: stacks.{key} is not a column's panels; ignored")),
        }
    }
    (out, stacks, warn)
}

fn render(layouts: &HashMap<Screen, Node<Pane>>, stacks: &Stacks) -> String {
    let state = State {
        layouts: layouts.iter().map(|(s, n)| (s.name().to_string(), n.clone())).collect(),
        stacks: stacks.iter().map(|(&(s, p), n)| (stack_key(s, p), n.clone())).collect(),
    };
    toml::to_string(&state).unwrap_or_default()
}

/// The writer thread. Sending is cheap; when several saves queue up while
/// one is written, only the newest goes to disk.
pub struct Saver {
    tx: Option<Sender<String>>,
}

impl Saver {
    /// `None` (no config directory) saves nothing.
    pub fn new(dir: Option<PathBuf>) -> Self {
        let Some(dir) = dir else { return Self { tx: None } };
        let (tx, rx) = unbounded::<String>();
        let spawned = std::thread::Builder::new().name("yagura-saver".into()).spawn(move || {
            while let Ok(mut text) = rx.recv() {
                while let Ok(newer) = rx.try_recv() {
                    text = newer;
                }
                if let Err(e) = write(&dir, &text) {
                    eprintln!("yagura: state.toml: {e}");
                }
            }
        });
        Self { tx: spawned.ok().map(|_| tx) }
    }

    pub fn save(&self, layouts: &HashMap<Screen, Node<Pane>>, stacks: &Stacks) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(render(layouts, stacks));
        }
    }
}

/// Write through a temporary file and a rename, so a crash never leaves half
/// a file.
fn write(dir: &Path, text: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join("state.toml.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, dir.join("state.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_dir;

    #[test]
    fn layouts_round_trip_and_bad_ones_fall_back() {
        let d = test_dir("persist");
        let (mut layouts, stacks, warn) = load(Some(&d));
        assert!(warn.is_empty() && stacks.is_empty());
        let mut cpu = Screen::Cpu.default_layout();
        assert!(cpu.nudge(&Pane::Left, ito_layout::Toward::Right, 0.1));
        layouts.insert(Screen::Cpu, cpu.clone());
        write(&d, &render(&layouts, &Stacks::new())).unwrap();
        let (back, _, warn) = load(Some(&d));
        assert!(warn.is_empty(), "{warn:?}");
        assert_eq!(back[&Screen::Cpu], cpu);
        assert_eq!(back[&Screen::Mem], Screen::Mem.default_layout());

        // A layout missing a column is replaced by the default.
        layouts.insert(Screen::Cpu, Node::Leaf(Pane::Left));
        write(&d, &render(&layouts, &Stacks::new())).unwrap();
        let (back, _, warn) = load(Some(&d));
        assert_eq!(warn.len(), 1, "{warn:?}");
        assert_eq!(back[&Screen::Cpu], Screen::Cpu.default_layout());

        std::fs::write(d.join("state.toml"), "layouts = 3").unwrap();
        let (back, _, warn) = load(Some(&d));
        assert_eq!(warn.len(), 1);
        assert_eq!(back.len(), Screen::ALL.len());
    }

    #[test]
    fn stacks_round_trip_and_strange_ones_are_dropped() {
        let d = test_dir("persist-stacks");
        let layouts: HashMap<_, _> = Screen::ALL.iter().map(|&s| (s, s.default_layout())).collect();
        let mut node = stacks::chain(ito_layout::Dir::Down, 0, 3);
        node.set_ratio(&[], 0.2);
        let mut saved = Stacks::new();
        saved.insert((Screen::Cpu, Pane::Mid), node.clone());
        write(&d, &render(&layouts, &saved)).unwrap();
        let (_, back, warn) = load(Some(&d));
        assert!(warn.is_empty(), "{warn:?}");
        assert_eq!(back[&(Screen::Cpu, Pane::Mid)], node);

        // A file from before the stacks reads as none; a strange key or
        // panels missing a number are dropped with a word.
        let old = test_dir("persist-old");
        std::fs::write(old.join("state.toml"), "layouts = {}\n").unwrap();
        let (_, back, warn) = load(Some(&old));
        assert!(back.is_empty() && warn.is_empty(), "{warn:?}");
        saved.insert((Screen::Mem, Pane::Mid), stacks::chain(ito_layout::Dir::Down, 1, 3));
        let mut text = render(&layouts, &saved);
        text += "\n[stacks.\"nowhere.mid\"]\nLeaf = 0\n";
        std::fs::write(d.join("state.toml"), text).unwrap();
        let (_, back, warn) = load(Some(&d));
        assert_eq!(warn.len(), 2, "{warn:?}");
        assert_eq!(back.len(), 1);
    }
}
