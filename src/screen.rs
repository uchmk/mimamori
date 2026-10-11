//! The screens behind the number keys (and `D`) and the three columns of each.

use ito_layout::{Dir, Node};
use serde::{Deserialize, Serialize};

/// One screen. The first ten have their number from the first version so
/// the keys never move; devices came later and opens with `D`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Screen {
    Ovw,
    Cpu,
    Mem,
    Gpu,
    Procs,
    Svc,
    Net,
    Disk,
    Sensors,
    Sys,
    Devices,
}

impl Screen {
    pub const ALL: [Screen; 11] = [
        Self::Ovw,
        Self::Cpu,
        Self::Mem,
        Self::Gpu,
        Self::Procs,
        Self::Svc,
        Self::Net,
        Self::Disk,
        Self::Sensors,
        Self::Sys,
        Self::Devices,
    ];

    /// The name in the keymap (`screen cpu`), in `state.toml` and in a
    /// scripted run's report.
    pub fn name(self) -> &'static str {
        match self {
            Self::Ovw => "overview",
            Self::Cpu => "cpu",
            Self::Mem => "mem",
            Self::Gpu => "gpu",
            Self::Procs => "procs",
            Self::Svc => "svc",
            Self::Net => "net",
            Self::Disk => "disk",
            Self::Sensors => "sensors",
            Self::Sys => "sys",
            Self::Devices => "devices",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|x| x.name() == s || (s == "ovw" && *x == Self::Ovw))
    }

    /// The key-like label on its tab: 1 to 9, then 0, then D.
    pub fn digit(self) -> char {
        match self.index() {
            9 => '0',
            10 => 'D',
            i => char::from(b'1' + i as u8),
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&x| x == self).unwrap_or(0)
    }

    /// Whether this version draws it; the others open on a "not yet" note.
    pub fn built(self) -> bool {
        matches!(
            self,
            Self::Ovw
                | Self::Cpu
                | Self::Mem
                | Self::Gpu
                | Self::Procs
                | Self::Svc
                | Self::Net
                | Self::Disk
                | Self::Sensors
                | Self::Sys
                | Self::Devices
        )
    }

    /// The `tr` key of its tab label.
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Ovw => "tab.overview",
            Self::Cpu => "tab.cpu",
            Self::Mem => "tab.mem",
            Self::Gpu => "tab.gpu",
            Self::Procs => "tab.procs",
            Self::Svc => "tab.svc",
            Self::Net => "tab.net",
            Self::Disk => "tab.disk",
            Self::Sensors => "tab.sensors",
            Self::Sys => "tab.sys",
            Self::Devices => "tab.devices",
        }
    }

    /// The three columns' first widths, the shares the mock's grid gave them.
    fn widths(self) -> [f32; 3] {
        match self {
            Self::Ovw => [1.15, 3.0, 1.85],
            Self::Cpu => [1.3, 3.0, 1.7],
            Self::Mem => [1.45, 2.8, 1.75],
            Self::Gpu => [1.7, 2.7, 1.9],
            Self::Procs => [0.9, 3.2, 2.0],
            Self::Svc => [0.9, 3.0, 2.1],
            Self::Net => [1.5, 3.2, 1.8],
            Self::Disk => [1.6, 2.8, 1.6],
            Self::Sensors => [2.0, 2.6, 1.6],
            Self::Sys => [1.9, 2.6, 1.9],
            Self::Devices => [1.4, 3.0, 2.0],
        }
    }

    /// The columns side by side at their first widths: left, then the middle
    /// and right split again, so each divider moves on its own.
    pub fn default_layout(self) -> Node<Pane> {
        let [a, b, c] = self.widths();
        Node::Split {
            dir: Dir::Right,
            ratio: a / (a + b + c),
            first: Box::new(Node::Leaf(Pane::Left)),
            second: Box::new(Node::Split {
                dir: Dir::Right,
                ratio: b / (b + c),
                first: Box::new(Node::Leaf(Pane::Mid)),
                second: Box::new(Node::Leaf(Pane::Right)),
            }),
        }
    }
}

/// A column of a screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Pane {
    Left,
    Mid,
    Right,
}

impl Pane {
    pub const ALL: [Pane; 3] = [Self::Left, Self::Mid, Self::Right];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Mid => "mid",
            Self::Right => "right",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_run_one_to_nine_then_zero() {
        let d: String = Screen::ALL.iter().map(|s| s.digit()).collect();
        assert_eq!(d, "1234567890D");
        assert_eq!(Screen::from_name("devices"), Some(Screen::Devices));
        assert_eq!(Screen::from_name("procs"), Some(Screen::Procs));
        assert_eq!(Screen::from_name("ovw"), Some(Screen::Ovw));
    }

    #[test]
    fn the_default_layout_has_the_mock_widths() {
        let rects = Screen::Ovw.default_layout().layout(ito_layout::Rect::new(0.0, 0.0, 600.0, 100.0), 0.0);
        let w: Vec<f32> = rects.iter().map(|(_, r)| r.w).collect();
        assert!((w[0] - 115.0).abs() < 0.5 && (w[1] - 300.0).abs() < 0.5 && (w[2] - 185.0).abs() < 0.5, "{w:?}");
    }
}
