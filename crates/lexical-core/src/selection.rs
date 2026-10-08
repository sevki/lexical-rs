use crate::format::TextFormat;
use crate::node::NodeKey;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(
    feature = "jetstream",
    derive(jetstream_wireformat::JetStreamWireFormat)
)]
pub enum PointKind {
    /// `offset` counts chars inside a text node.
    Text,
    /// `offset` is a child index inside an element.
    Element,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(
    feature = "jetstream",
    derive(jetstream_wireformat::JetStreamWireFormat)
)]
pub struct Point {
    pub key: NodeKey,
    pub offset: usize,
    pub kind: PointKind,
}

impl Point {
    pub fn text(key: NodeKey, offset: usize) -> Point {
        Point {
            key,
            offset,
            kind: PointKind::Text,
        }
    }
    pub fn element(key: NodeKey, offset: usize) -> Point {
        Point {
            key,
            offset,
            kind: PointKind::Element,
        }
    }
}

/// A range selection. `format`/`style` are the pending formatting applied to the
/// next typed text (mirrors Lexical's `RangeSelection.format`).
#[derive(Clone, PartialEq, Debug)]
#[cfg_attr(
    feature = "jetstream",
    derive(jetstream_wireformat::JetStreamWireFormat)
)]
pub struct Selection {
    pub anchor: Point,
    pub focus: Point,
    pub format: TextFormat,
    pub style: String,
}

impl Selection {
    pub fn new(anchor: Point, focus: Point) -> Selection {
        Selection {
            anchor,
            focus,
            format: TextFormat::empty(),
            style: String::new(),
        }
    }
    pub fn collapsed(p: Point) -> Selection {
        Selection::new(p, p)
    }
    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.focus
    }
}
