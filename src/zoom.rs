//! A graph opened full size (double click): which history it shows, how the
//! numbers read, and how far back it looks.

use egui::Color32;

use crate::collect::sensors::Kind;
use crate::model::{Range, Series};

/// How a zoomed graph's values read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Unit {
    Pct,
    /// Bytes per second.
    Rate,
    Sensor(Kind),
}

/// What a graph asks for when it is double clicked.
#[derive(Clone, Debug)]
pub struct Zoom {
    pub series: Series,
    pub title: String,
    pub color: Color32,
    pub unit: Unit,
    /// The top of the scale; `None` scales to the range.
    pub max: Option<f32>,
    pub range: Range,
}

impl Zoom {
    pub fn new(series: Series, title: impl Into<String>, color: Color32, unit: Unit) -> Self {
        let max = (unit == Unit::Pct).then_some(100.0);
        Self { series, title: title.into(), color, unit, max, range: Range::default() }
    }
}
