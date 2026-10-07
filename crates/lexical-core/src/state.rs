//! The immutable-by-convention document snapshot plus all raw tree operations.

use crate::error::{Error, Result};
use crate::format::{Align, TextFormat};
use crate::node::*;
use crate::selection::{Point, PointKind, Selection};
use std::collections::{BTreeSet, HashMap};

#[derive(Clone, Debug)]
pub struct EditorState {
    pub(crate) nodes: HashMap<NodeKey, Node>,
    pub selection: Option<Selection>,
    next_key: u64,
    pub(crate) dirty: BTreeSet<NodeKey>,
}

impl Default for EditorState {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorState {
    /// A document with a root and a single empty paragraph, caret inside it.
    pub fn new() -> EditorState {
        let mut s = EditorState::empty();
        let p = s.create_node(NodeData::Paragraph);
        s.append_child(ROOT_KEY, p);
        s.selection = Some(Selection::collapsed(Point::element(p, 0)));
        s.dirty.clear();
        s
    }

    /// Only a root node, no selection.
    pub fn empty() -> EditorState {
        let mut nodes = HashMap::new();
        nodes.insert(
            ROOT_KEY,
            Node {
                key: ROOT_KEY,
                parent: None,
                children: vec![],
                data: NodeData::Root,
                indent: 0,
                align: Align::Start,
            },
        );
        EditorState { nodes, selection: None, next_key: 1, dirty: BTreeSet::new() }
    }

    // ---------------------------------------------------------------- lookup

    pub fn get(&self, key: NodeKey) -> Option<&Node> {
        self.nodes.get(&key)
    }

    /// Panics if the node is missing; use [`get`](Self::get) for untrusted keys.
    pub fn node(&self, key: NodeKey) -> &Node {
        self.nodes.get(&key).unwrap_or_else(|| panic!("node {key} does not exist"))
    }

    pub(crate) fn node_mut(&mut self, key: NodeKey) -> &mut Node {
        self.mark_dirty(key);
        self.nodes.get_mut(&key).unwrap_or_else(|| panic!("node {key} does not exist"))
    }

    pub fn contains(&self, key: NodeKey) -> bool {
        self.nodes.contains_key(&key)
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    pub fn children(&self, key: NodeKey) -> &[NodeKey] {
        &self.node(key).children
    }

    pub fn parent(&self, key: NodeKey) -> Option<NodeKey> {
        self.get(key).and_then(|n| n.parent)
    }

    pub fn index_in_parent(&self, key: NodeKey) -> Option<usize> {
        let p = self.parent(key)?;
        self.node(p).children.iter().position(|&c| c == key)
    }

    pub fn next_sibling(&self, key: NodeKey) -> Option<NodeKey> {
        let p = self.parent(key)?;
        let i = self.index_in_parent(key)?;
        self.node(p).children.get(i + 1).copied()
    }

    pub fn prev_sibling(&self, key: NodeKey) -> Option<NodeKey> {
        let p = self.parent(key)?;
        let i = self.index_in_parent(key)?;
        i.checked_sub(1).map(|j| self.node(p).children[j])
    }

    /// `key`'s ancestors, nearest first, ending at the root.
    pub fn ancestors(&self, key: NodeKey) -> Vec<NodeKey> {
        let mut out = vec![];
        let mut cur = self.parent(key);
        while let Some(p) = cur {
            out.push(p);
            cur = self.parent(p);
        }
        out
    }

    pub fn is_ancestor_or_self(&self, anc: NodeKey, key: NodeKey) -> bool {
        let mut cur = Some(key);
        while let Some(c) = cur {
            if c == anc {
                return true;
            }
            cur = self.parent(c);
        }
        false
    }

    pub fn root_children(&self) -> &[NodeKey] {
        self.children(ROOT_KEY)
    }

    // ----------------------------------------------------------- dirty marks

    pub(crate) fn mark_dirty(&mut self, key: NodeKey) {
        self.dirty.insert(key);
        if let Some(p) = self.nodes.get(&key).and_then(|n| n.parent) {
            self.dirty.insert(p);
        }
    }

    pub fn dirty_nodes(&self) -> impl Iterator<Item = NodeKey> + '_ {
        self.dirty.iter().copied().filter(|k| self.nodes.contains_key(k))
    }

    // ------------------------------------------------------------- creation

    pub fn create_node(&mut self, data: NodeData) -> NodeKey {
        let key = NodeKey(self.next_key);
        self.next_key += 1;
        self.nodes.insert(
            key,
            Node { key, parent: None, children: vec![], data, indent: 0, align: Align::Start },
        );
        self.dirty.insert(key);
        key
    }

    pub fn create_text(&mut self, text: &str) -> NodeKey {
        self.create_node(NodeData::text(text, TextFormat::empty()))
    }

    pub fn create_paragraph(&mut self) -> NodeKey {
        self.create_node(NodeData::Paragraph)
    }

    // ------------------------------------------------------------ tree edits

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

    // -------------------------------------------------------------- text ops

    pub fn set_text(&mut self, key: NodeKey, new_text: &str) {
        if let NodeData::Text { text, .. } = &mut self.node_mut(key).data {
            *text = new_text.to_string();
        }
    }

    pub fn set_text_format(&mut self, key: NodeKey, f: TextFormat) {
        if let NodeData::Text { format, .. } = &mut self.node_mut(key).data {
            *format = f;
        }
    }

    /// Split a text node at the given char offsets (ascending). Offsets at 0 or the
    /// end are ignored. Returns all resulting pieces; the first keeps `key`.
    /// Selection points move with the characters.
    pub fn split_text(&mut self, key: NodeKey, offsets: &[usize]) -> Vec<NodeKey> {
        let node = self.node(key);
        let NodeData::Text { text, format, style, mode, detail } = node.data.clone() else {
            return vec![key];
        };
        let len = text.chars().count();
        let mut cuts: Vec<usize> =
            offsets.iter().copied().filter(|&o| o > 0 && o < len).collect();
        cuts.dedup();
        if cuts.is_empty() {
            return vec![key];
        }
        let mut bounds = vec![0];
        bounds.extend(&cuts);
        bounds.push(len);
        let chars: Vec<char> = text.chars().collect();
        let piece = |i: usize| -> String { chars[bounds[i]..bounds[i + 1]].iter().collect() };

        self.set_text(key, &piece(0));
        let mut pieces = vec![key];
        let mut prev = key;
        for i in 1..bounds.len() - 1 {
            let n = self.create_node(NodeData::Text {
                text: piece(i),
                format,
                style: style.clone(),
                mode,
                detail,
            });
            self.insert_after(prev, n);
            pieces.push(n);
            prev = n;
        }
        if let Some(sel) = self.selection.as_mut() {
            for p in [&mut sel.anchor, &mut sel.focus] {
                if p.kind == PointKind::Text && p.key == key {
                    let i = (0..pieces.len())
                        .find(|&i| p.offset <= bounds[i + 1])
                        .unwrap_or(pieces.len() - 1);
                    p.key = pieces[i];
                    p.offset -= bounds[i];
                }
            }
        }
        pieces
    }

    // ------------------------------------------------------------ structure

    pub fn is_line_block(&self, key: NodeKey) -> bool {
        let n = self.node(key);
        match n.data {
            NodeData::Paragraph | NodeData::Heading(_) | NodeData::Quote | NodeData::Code { .. } => true,
            NodeData::ListItem { .. } => !n.children.iter().any(|&c| self.node(c).node_type() == NodeType::List),
            _ => false,
        }
    }

    /// Nearest ancestor-or-self that is a line block.
    pub fn line_block_of(&self, key: NodeKey) -> Option<NodeKey> {
        let mut cur = Some(key);
        while let Some(c) = cur {
            if self.is_line_block(c) {
                return Some(c);
            }
            cur = self.parent(c);
        }
        None
    }

    /// All line blocks in document order.
    pub fn line_blocks(&self) -> Vec<NodeKey> {
        let mut out = vec![];
        self.collect_blocks(ROOT_KEY, &mut out);
        out
    }

    fn collect_blocks(&self, key: NodeKey, out: &mut Vec<NodeKey>) {
        for &c in &self.node(key).children {
            if self.is_line_block(c) {
                out.push(c);
            } else if self.node(c).is_element() && !self.node(c).is_inline() {
                self.collect_blocks(c, out);
            }
        }
    }

    /// Pre-order index of every node; the second value is `key -> end of subtree`.
    pub(crate) fn preorder(&self) -> (HashMap<NodeKey, usize>, HashMap<NodeKey, usize>) {
        fn walk(
            s: &EditorState,
            k: NodeKey,
            n: &mut usize,
            pre: &mut HashMap<NodeKey, usize>,
            end: &mut HashMap<NodeKey, usize>,
        ) {
            pre.insert(k, *n);
            *n += 1;
            for &c in &s.node(k).children {
                walk(s, c, n, pre, end);
            }
            end.insert(k, *n);
        }
        let (mut pre, mut end, mut n) = (HashMap::new(), HashMap::new(), 0);
        walk(self, ROOT_KEY, &mut n, &mut pre, &mut end);
        (pre, end)
    }

    /// Text and linebreak leaves in document order.
    pub fn leaves(&self) -> Vec<NodeKey> {
        let mut out = vec![];
        fn walk(s: &EditorState, k: NodeKey, out: &mut Vec<NodeKey>) {
            for &c in &s.node(k).children {
                let n = s.node(c);
                if n.is_element() {
                    walk(s, c, out)
                } else {
                    out.push(c)
                }
            }
        }
        walk(self, ROOT_KEY, &mut out);
        out
    }

    /// Lexical-style text content: block children are separated by a blank line.
    pub fn text_content(&self, key: NodeKey) -> String {
        let n = self.node(key);
        match &n.data {
            NodeData::Text { text, .. } => text.clone(),
            NodeData::LineBreak => "\n".into(),
            _ => {
                let mut out = String::new();
                let last = n.children.len().saturating_sub(1);
                for (i, &c) in n.children.iter().enumerate() {
                    out.push_str(&self.text_content(c));
                    let cn = self.node(c);
                    if cn.is_element() && !cn.is_inline() && i != last {
                        out.push_str("\n\n");
                    }
                }
                out
            }
        }
    }

    // ------------------------------------------------------------- selection

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
        Ok(if self.point_cmp(&sel.anchor, &sel.focus) == std::cmp::Ordering::Greater {
            (sel.focus, sel.anchor)
        } else {
            (sel.anchor, sel.focus)
        })
    }

    pub fn is_backward(&self) -> bool {
        self.selection
            .as_ref()
            .is_some_and(|s| self.point_cmp(&s.anchor, &s.focus) == std::cmp::Ordering::Greater)
    }

    /// Set a collapsed selection and refresh pending format from the node there.
    pub fn set_caret(&mut self, p: Point) {
        let fmt = self.format_at(&p);
        let sel = self.selection.get_or_insert_with(|| Selection::collapsed(p));
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
            self.get(p.key).filter(|n| n.is_text()).map(|n| n.text_format())
        } else {
            None
        }
    }

    /// Make sure selection points reference live nodes with in-range offsets.
    pub(crate) fn validate_selection(&mut self) {
        let Some(sel) = self.selection.clone() else { return };
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
