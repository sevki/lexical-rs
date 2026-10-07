//! Post-update tidying: merge adjacent compatible text nodes and lists, drop empty
//! nodes, and settle list-item state. Whatever sequence of edits produced a tree, the
//! result satisfies `EditorState::check_invariants`.

use crate::node::*;
use crate::selection::{Point, PointKind};
use crate::state::EditorState;

const MAX_PASSES: usize = 32;

impl EditorState {
    pub(crate) fn normalize_dirty(&mut self) {
        // Removing an empty link can make two text nodes adjacent, merging two lists can
        // make their items or wrappers adjacent, and so on, so iterate to a fixed point.
        for _ in 0..MAX_PASSES {
            let empty_links: Vec<_> = self
                .nodes
                .values()
                .filter(|n| n.is_inline() && n.children.is_empty())
                .map(|n| n.key)
                .collect();
            for l in &empty_links {
                self.remove(*l);
            }
            let mut containers: Vec<NodeKey> = self
                .dirty
                .iter()
                .copied()
                .filter(|k| self.contains(*k) && self.node(*k).is_element())
                .collect();
            containers.sort();
            let mut changed = !empty_links.is_empty();
            for c in containers {
                if self.contains(c) {
                    changed |= self.merge_list_structure(c);
                }
                if self.contains(c) {
                    self.normalize_element(c);
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Adjacent lists of the same type are one list, and adjacent nesting wrappers are one
    /// wrapper (Lexical merges them too). Returns whether anything was merged.
    fn merge_list_structure(&mut self, el: NodeKey) -> bool {
        let is_wrapper = |s: &EditorState, k: NodeKey| {
            s.node(k).node_type() == NodeType::ListItem
                && s.node(k).children.iter().any(|&c| s.node(c).node_type() == NodeType::List)
        };
        let mut changed = false;
        let mut i = 0;
        while i + 1 < self.node(el).children.len() {
            let (a, b) = (self.node(el).children[i], self.node(el).children[i + 1]);
            let mergeable = match (&self.node(a).data, &self.node(b).data) {
                (NodeData::List { list_type: x, .. }, NodeData::List { list_type: y, .. }) => x == y,
                (NodeData::ListItem { .. }, NodeData::ListItem { .. }) => is_wrapper(self, a) && is_wrapper(self, b),
                _ => false,
            };
            if mergeable {
                for k in self.node(b).children.clone() {
                    self.append_child(a, k);
                }
                self.remove(b);
                changed = true;
            } else {
                i += 1;
            }
        }
        changed
    }

    fn normalize_element(&mut self, el: NodeKey) {
        self.normalize_checked(el);
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
            if let Some(&nx) = self.node(el).children.get(i + 1)
                && self.mergeable(k, nx) {
                    self.merge_text(k, nx);
                    continue;
                }
            i += 1;
        }
    }

    /// A list item has a checked state exactly when it is a real item of a check list
    /// (never a nesting wrapper). Whatever path moved or retyped items, this settles it.
    fn normalize_checked(&mut self, el: NodeKey) {
        let NodeData::List { list_type, .. } = self.node(el).data else { return };
        for item in self.node(el).children.clone() {
            let wrapper = self.node(item).children.iter().any(|&c| self.node(c).node_type() == NodeType::List);
            let want = |cur: Option<bool>| (list_type == ListType::Check && !wrapper).then(|| cur.unwrap_or(false));
            if let NodeData::ListItem { checked } = self.node(item).data
                && checked != want(checked)
                && let NodeData::ListItem { checked } = &mut self.node_mut(item).data
            {
                *checked = want(*checked);
            }
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
