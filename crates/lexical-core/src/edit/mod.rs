//! Selection-driven editing. This module holds the shared notion of a caret
//! *boundary* (a gap between two children, created by splitting text nodes);
//! typing, deleting, paragraphs and formatting are built on it in the sibling modules.

mod delete;
mod format;
mod insert;
mod paragraph;

pub use delete::Granularity;

use crate::node::*;
use crate::selection::{Point, PointKind};
use crate::state::EditorState;
use std::collections::HashMap;

/// A caret position between two children of `parent`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Boundary {
    pub parent: NodeKey,
    pub before: Option<NodeKey>,
}

impl EditorState {
    pub(crate) fn boundary_index(&self, b: &Boundary) -> usize {
        match b.before {
            Some(k) => self.index_in_parent(k).expect("boundary node detached"),
            None => self.node(b.parent).children.len(),
        }
    }

    /// Turn a point into a boundary, splitting a text node when the point is inside it.
    pub(crate) fn boundary_from_point(&mut self, p: &Point) -> Boundary {
        let p = self.inline_point(p);
        match p.kind {
            PointKind::Element => {
                let kids = &self.node(p.key).children;
                Boundary { parent: p.key, before: kids.get(p.offset).copied() }
            }
            PointKind::Text => {
                let len = self.node(p.key).text_len();
                let parent = self.parent(p.key).expect("text without parent");
                if p.offset == 0 {
                    Boundary { parent, before: Some(p.key) }
                } else if p.offset >= len {
                    Boundary { parent, before: self.next_sibling(p.key) }
                } else {
                    let pieces = self.split_text(p.key, &[p.offset]);
                    Boundary { parent, before: Some(pieces[1]) }
                }
            }
        }
    }

    /// Split the range's text nodes at both ends; returns the two boundaries.
    pub(crate) fn split_range(&mut self, start: &Point, end: &Point) -> (Boundary, Boundary) {
        let b = self.boundary_from_point(end);
        let a = self.boundary_from_point(start);
        (a, b)
    }

    fn boundary_pos(
        &self,
        b: &Boundary,
        pre: &HashMap<NodeKey, usize>,
        end: &HashMap<NodeKey, usize>,
    ) -> usize {
        match b.before {
            Some(k) => pre[&k],
            None => end[&b.parent],
        }
    }

    /// Leaves (text/linebreak) between two boundaries, in document order.
    pub(crate) fn leaves_between(&self, a: &Boundary, b: &Boundary) -> Vec<NodeKey> {
        let (pre, end) = self.preorder();
        let (pa, pb) = (self.boundary_pos(a, &pre, &end), self.boundary_pos(b, &pre, &end));
        self.leaves().into_iter().filter(|k| pre[k] >= pa && pre[k] < pb).collect()
    }

    /// Prefer a text point next to `(parent, idx)` over a bare element point.
    fn prefer_text_point(&self, parent: NodeKey, idx: usize) -> Point {
        let kids = &self.node(parent).children;
        let idx = idx.min(kids.len());
        if let Some(&p) = idx.checked_sub(1).and_then(|i| kids.get(i))
            && self.node(p).is_text()
        {
            return Point::text(p, self.node(p).text_len());
        }
        if let Some(&n) = kids.get(idx)
            && self.node(n).is_text()
        {
            return Point::text(n, 0);
        }
        Point::element(parent, idx)
    }
}
