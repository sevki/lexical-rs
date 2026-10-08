//! Flattened inline content of a line block and point <-> offset mapping.
//! This is the Rust analogue of Lexical iOS's range cache, scoped to one block.

use crate::node::*;
use crate::selection::{Point, PointKind};
use crate::state::EditorState;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct Piece {
    pub key: NodeKey,
    pub start: usize,
    pub len: usize,
    pub is_text: bool,
}

#[derive(Clone, Debug, Default)]
pub struct BlockContent {
    /// Block text; line breaks are `\n`.
    pub text: String,
    pub pieces: Vec<Piece>,
    /// For each element, the start offset of every child plus the end offset.
    child_pos: HashMap<NodeKey, Vec<usize>>,
    pub len: usize,
}

impl EditorState {
    pub fn block_content(&self, block: NodeKey) -> BlockContent {
        let mut c = BlockContent::default();
        self.walk_content(block, &mut c);
        c.len = c.text.chars().count();
        c
    }

    fn walk_content(&self, el: NodeKey, c: &mut BlockContent) {
        let mut starts = vec![];
        for &k in &self.node(el).children {
            let n = self.node(k);
            let start = c.text.chars().count();
            starts.push(start);
            match &n.data {
                NodeData::Text { text, .. } => {
                    c.text.push_str(text);
                    c.pieces.push(Piece { key: k, start, len: n.text_len(), is_text: true });
                }
                NodeData::LineBreak => {
                    c.text.push('\n');
                    c.pieces.push(Piece { key: k, start, len: 1, is_text: false });
                }
                _ if n.is_element() => {
                    self.walk_content(k, c);
                }
                _ => {}
            }
        }
        starts.push(c.text.chars().count());
        c.child_pos.insert(el, starts);
    }

    /// Offset of `p` within `block`'s content.
    pub fn block_offset(&self, content: &BlockContent, p: &Point) -> usize {
        match p.kind {
            PointKind::Text => content
                .pieces
                .iter()
                .find(|x| x.key == p.key)
                .map_or(content.len, |x| x.start + p.offset.min(x.len)),
            PointKind::Element => content
                .child_pos
                .get(&p.key)
                .map_or(content.len, |v| v[p.offset.min(v.len() - 1)]),
        }
    }

    /// Point for `offset`; prefers the end of an earlier text node over the start
    /// of the next, and element points next to line breaks.
    pub fn block_point(&self, block: NodeKey, content: &BlockContent, offset: usize) -> Point {
        let offset = offset.min(content.len);
        if let Some(x) = content
            .pieces
            .iter()
            .find(|x| x.is_text && offset >= x.start && offset <= x.start + x.len)
        {
            return Point::text(x.key, offset - x.start);
        }
        if let Some(x) = content.pieces.iter().find(|x| !x.is_text && x.start == offset) {
            let par = self.parent(x.key).unwrap();
            return Point::element(par, self.index_in_parent(x.key).unwrap());
        }
        if let Some(x) = content.pieces.last()
            && !x.is_text && x.start + x.len == offset {
                let par = self.parent(x.key).unwrap();
                return Point::element(par, self.index_in_parent(x.key).unwrap() + 1);
            }
        Point::element(block, 0)
    }

    /// Resolve a point on a non-inline container (root, list, list wrapper) to a
    /// point inside a line block.
    pub fn inline_point(&self, p: &Point) -> Point {
        let mut p = *p;
        loop {
            let Some(n) = self.get(p.key) else { return p };
            if p.kind == PointKind::Text || !n.is_element() || n.is_inline() || self.is_line_block(p.key) {
                return p;
            }
            if n.children.is_empty() {
                return p;
            }
            if let Some(&c) = n.children.get(p.offset) {
                p = Self::start_of(self, c);
            } else {
                let c = *n.children.last().unwrap();
                p = Self::end_of(self, c);
            }
        }
    }

    fn start_of(s: &EditorState, c: NodeKey) -> Point {
        if s.node(c).is_element() {
            Point::element(c, 0)
        } else {
            // leaf directly under a block never happens for root children, be safe
            Point::text(c, 0)
        }
    }

    fn end_of(s: &EditorState, c: NodeKey) -> Point {
        let n = s.node(c);
        if n.is_element() {
            Point::element(c, n.children.len())
        } else {
            Point::text(c, n.text_len())
        }
    }
}
