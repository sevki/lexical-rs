//! Raw tree edits. Selection points are kept consistent as nodes move or disappear.

use super::EditorState;
use crate::node::*;
use crate::selection::{Point, PointKind};

impl EditorState {
    /// Detach `key` from its parent. The subtree stays alive so it can be re-linked.
    pub(crate) fn unlink(&mut self, key: NodeKey) {
        let Some(parent) = self.parent(key) else { return };
        let idx = self.index_in_parent(key).expect("child missing from parent");
        self.mark_dirty(key);
        self.dirty.insert(parent);
        self.nodes.get_mut(&parent).unwrap().children.remove(idx);
        self.nodes.get_mut(&key).unwrap().parent = None;
        self.shift_element_points(parent, idx + 1, -1);
    }

    pub(crate) fn link_at(&mut self, parent: NodeKey, index: usize, child: NodeKey) {
        debug_assert!(self.node(child).parent.is_none(), "child must be detached");
        let idx = index.min(self.node(parent).children.len());
        self.nodes.get_mut(&parent).unwrap().children.insert(idx, child);
        self.nodes.get_mut(&child).unwrap().parent = Some(parent);
        self.dirty.insert(parent);
        self.dirty.insert(child);
        self.shift_element_points(parent, idx + 1, 1);
    }

    /// Move/insert `child` (detaching it first) at `index` inside `parent`.
    pub fn insert_child(&mut self, parent: NodeKey, index: usize, child: NodeKey) {
        self.unlink(child);
        self.link_at(parent, index, child);
    }

    pub fn append_child(&mut self, parent: NodeKey, child: NodeKey) {
        let n = self.node(parent).children.len();
        self.insert_child(parent, n, child);
    }

    pub fn insert_after(&mut self, sibling: NodeKey, new: NodeKey) {
        let p = self.parent(sibling).expect("sibling has no parent");
        let i = self.index_in_parent(sibling).unwrap();
        self.unlink(new);
        self.link_at(p, i + 1, new);
    }

    pub fn insert_before(&mut self, sibling: NodeKey, new: NodeKey) {
        let p = self.parent(sibling).expect("sibling has no parent");
        let i = self.index_in_parent(sibling).unwrap();
        self.unlink(new);
        self.link_at(p, i, new);
    }

    /// Delete `key` and its subtree; selection points inside collapse to the gap.
    pub fn remove(&mut self, key: NodeKey) {
        if key == ROOT_KEY || !self.contains(key) {
            return;
        }
        let parent = self.parent(key);
        let idx = self.index_in_parent(key).unwrap_or(0);
        let (mut a_in, mut f_in) = (false, false);
        if let Some(sel) = &self.selection {
            a_in = self.is_ancestor_or_self(key, sel.anchor.key);
            f_in = self.is_ancestor_or_self(key, sel.focus.key);
        }
        self.unlink(key);
        let mut stack = vec![key];
        while let Some(k) = stack.pop() {
            if let Some(n) = self.nodes.remove(&k) {
                stack.extend(n.children);
            }
            self.dirty.insert(k);
        }
        if let (Some(parent), Some(sel)) = (parent, self.selection.as_mut()) {
            if a_in {
                sel.anchor = Point::element(parent, idx);
            }
            if f_in {
                sel.focus = Point::element(parent, idx);
            }
        }
    }

    /// Move `old`'s children into `new` and retarget selection points on `old`.
    /// Neither node's own position in the tree changes.
    pub(crate) fn transfer_children(&mut self, old: NodeKey, new: NodeKey) {
        let kids: Vec<NodeKey> = self.node(old).children.clone();
        for k in kids {
            self.append_child(new, k);
        }
        if let Some(sel) = self.selection.as_mut() {
            for p in [&mut sel.anchor, &mut sel.focus] {
                if p.key == old {
                    p.key = new;
                }
            }
        }
    }

    /// Replace element `old` by the detached element `new`, in place.
    pub(crate) fn replace_element(&mut self, old: NodeKey, new: NodeKey) {
        self.insert_after(old, new);
        self.transfer_children(old, new);
        self.remove(old);
    }

    fn shift_element_points(&mut self, parent: NodeKey, from: usize, delta: isize) {
        if let Some(sel) = self.selection.as_mut() {
            for p in [&mut sel.anchor, &mut sel.focus] {
                if p.kind == PointKind::Element && p.key == parent && p.offset >= from {
                    p.offset = (p.offset as isize + delta).max(0) as usize;
                }
            }
        }
    }
}
