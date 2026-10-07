//! Document-structure queries: line blocks, leaves, pre-order, text content.

use super::EditorState;
use crate::node::*;
use std::collections::HashMap;

impl EditorState {
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
}
