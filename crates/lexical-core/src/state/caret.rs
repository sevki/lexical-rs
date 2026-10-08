//! Selection ordering, caret placement and validation.

use super::EditorState;
use crate::error::{Error, Result};
use crate::format::TextFormat;
use crate::node::*;
use crate::selection::{Point, PointKind, Selection};

impl EditorState {
    pub fn require_selection(&self) -> Result<Selection> {
        self.selection.clone().ok_or(Error::NoSelection)
    }

    /// Compare two points in document order.
    pub fn point_cmp(&self, a: &Point, b: &Point) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        if a.key == b.key {
            return a.offset.cmp(&b.offset);
        }
        let path = |k: NodeKey| -> Vec<usize> {
            let mut p = vec![];
            let mut cur = k;
            while let Some(par) = self.parent(cur) {
                p.push(self.index_in_parent(cur).unwrap());
                cur = par;
            }
            p.reverse();
            p
        };
        let (pa, pb) = (path(a.key), path(b.key));
        let anc = |anc_pt: &Point, anc_path: &[usize], other: &[usize]| -> Option<bool> {
            // returns Some(true) if anc_pt (an element) sits before `other`
            if anc_pt.kind == PointKind::Element
                && other.len() > anc_path.len()
                && other[..anc_path.len()] == *anc_path
            {
                Some(anc_pt.offset <= other[anc_path.len()])
            } else {
                None
            }
        };
        if let Some(before) = anc(a, &pa, &pb) {
            return if before { Less } else { Greater };
        }
        if let Some(before) = anc(b, &pb, &pa) {
            return if before { Greater } else { Less };
        }
        pa.cmp(&pb)
    }

    /// `(start, end)` of the selection in document order.
    pub fn ordered_points(&self) -> Result<(Point, Point)> {
        let sel = self.require_selection()?;
        Ok(
            if self.point_cmp(&sel.anchor, &sel.focus) == std::cmp::Ordering::Greater {
                (sel.focus, sel.anchor)
            } else {
                (sel.anchor, sel.focus)
            },
        )
    }

    pub fn is_backward(&self) -> bool {
        self.selection
            .as_ref()
            .is_some_and(|s| self.point_cmp(&s.anchor, &s.focus) == std::cmp::Ordering::Greater)
    }

    /// Set a collapsed selection and refresh pending format from the node there.
    pub fn set_caret(&mut self, p: Point) {
        let fmt = self.format_at(&p);
        let sel = self
            .selection
            .get_or_insert_with(|| Selection::collapsed(p));
        sel.anchor = p;
        sel.focus = p;
        if let Some(f) = fmt {
            sel.format = f;
        }
    }

    pub fn set_selection_points(&mut self, anchor: Point, focus: Point) {
        let old = self.selection.take();
        let mut sel = Selection::new(anchor, focus);
        if let Some(o) = old {
            sel.format = o.format;
            sel.style = o.style;
        }
        self.selection = Some(sel);
        if anchor == focus {
            self.set_caret(anchor);
        } else if let Some(f) = self.format_at(&anchor) {
            self.selection.as_mut().unwrap().format = f;
        }
    }

    fn format_at(&self, p: &Point) -> Option<TextFormat> {
        if p.kind == PointKind::Text {
            self.get(p.key)
                .filter(|n| n.is_text())
                .map(|n| n.text_format())
        } else {
            None
        }
    }

    /// Make sure selection points reference live nodes with in-range offsets.
    pub(crate) fn validate_selection(&mut self) {
        let Some(sel) = self.selection.clone() else {
            return;
        };
        let fix = |s: &EditorState, p: Point| -> Point {
            match s.get(p.key) {
                Some(n) if p.kind == PointKind::Text && n.is_text() => {
                    Point::text(p.key, p.offset.min(n.text_len()))
                }
                Some(n) if p.kind == PointKind::Element && n.is_element() => {
                    Point::element(p.key, p.offset.min(n.children.len()))
                }
                _ => match s.line_blocks().last() {
                    Some(&b) => Point::element(b, s.node(b).children.len()),
                    None => Point::element(ROOT_KEY, s.node(ROOT_KEY).children.len()),
                },
            }
        };
        let (a, f) = (fix(self, sel.anchor), fix(self, sel.focus));
        let s = self.selection.as_mut().unwrap();
        s.anchor = a;
        s.focus = f;
    }
}
