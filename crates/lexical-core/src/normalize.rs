//! Post-update tidying: merge adjacent compatible text nodes, drop empty ones.

use crate::node::*;
use crate::selection::{Point, PointKind};
use crate::state::EditorState;

impl EditorState {
    pub(crate) fn normalize_dirty(&mut self) {
        let mut containers: Vec<NodeKey> = self
            .dirty
            .iter()
            .copied()
            .filter(|k| self.contains(*k) && self.node(*k).is_element())
            .collect();
        containers.sort();
        for c in containers {
            if self.contains(c) {
                self.normalize_element(c);
            }
        }
        // Empty links left behind by deletions.
        let empty_links: Vec<_> = self
            .nodes
            .values()
            .filter(|n| n.is_inline() && n.children.is_empty())
            .map(|n| n.key)
            .collect();
        for l in empty_links {
            self.remove(l);
        }
    }

    fn normalize_element(&mut self, el: NodeKey) {
        let mut i = 0;
        while i < self.node(el).children.len() {
            let k = self.node(el).children[i];
            if !self.node(k).is_text() {
                i += 1;
                continue;
            }
            if self.node(k).text_len() == 0 && !self.selection_needs(k) {
                self.remove(k);
                continue;
            }
            if let Some(&nx) = self.node(el).children.get(i + 1) {
                if self.mergeable(k, nx) {
                    self.merge_text(k, nx);
                    continue;
                }
            }
            i += 1;
        }
    }

    /// Empty text nodes are kept only while the caret sits in them with no sibling to go to.
    fn selection_needs(&self, _k: NodeKey) -> bool {
        false
    }

    fn mergeable(&self, a: NodeKey, b: NodeKey) -> bool {
        let (na, nb) = (self.node(a), self.node(b));
        match (&na.data, &nb.data) {
            (
                NodeData::Text { format: fa, style: sa, mode: ma, .. },
                NodeData::Text { format: fb, style: sb, mode: mb, .. },
            ) => fa == fb && sa == sb && ma == mb && *ma == TextMode::Normal && *mb == TextMode::Normal,
            _ => false,
        }
    }

    /// Append `b` onto `a` and delete `b`, keeping selection points on the same chars.
    fn merge_text(&mut self, a: NodeKey, b: NodeKey) {
        let la = self.node(a).text_len();
        let joined = format!("{}{}", self.node(a).text().unwrap(), self.node(b).text().unwrap());
        self.set_text(a, &joined);
        if let Some(sel) = self.selection.as_mut() {
            for p in [&mut sel.anchor, &mut sel.focus] {
                if p.kind == PointKind::Text && p.key == b {
                    *p = Point::text(a, p.offset + la);
                }
            }
        }
        // `remove` would collapse points still on `b`; none remain after the loop above.
        self.remove(b);
    }
}
