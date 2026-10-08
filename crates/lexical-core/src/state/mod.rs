//! The immutable-by-convention document snapshot: an arena of nodes addressed by key.
//! Tree edits, text splitting, document structure queries and caret handling live in
//! the sibling modules.

mod caret;
mod structure;
mod text;
mod tree;

use crate::format::{Align, TextFormat};
use crate::limits::Limits;
use crate::node::*;
use crate::selection::{Point, Selection};
use std::collections::{BTreeSet, HashMap};

///
/// With the `jetstream` feature this type is a JetStream `WireFormat`. Decoding does not
/// check the arena, so bytes from an untrusted source must go through
/// [`EditorState::from_wire_bytes`] (or be followed by [`EditorState::check_wire`]).
#[derive(Clone, Debug)]
#[cfg_attr(feature = "jetstream", derive(jetstream_wireformat::JetStreamWireFormat))]
pub struct EditorState {
    // Encoded in key order (canonical bytes) with a `u32` count, not JetStream's `u16`.
    #[cfg_attr(feature = "jetstream", jetstream(with(crate::wire::wide::Map)))]
    pub(crate) nodes: HashMap<NodeKey, Node>,
    pub selection: Option<Selection>,
    /// Optional editing caps; host configuration rather than document content, so it
    /// is not serialized and survives `Editor::set_state`.
    #[cfg_attr(feature = "jetstream", jetstream(skip))]
    pub limits: Limits,
    next_key: u64,
    #[cfg_attr(feature = "jetstream", jetstream(skip))]
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
        EditorState {
            nodes,
            selection: None,
            limits: Limits::default(),
            next_key: 1,
            dirty: BTreeSet::new(),
        }
    }

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

    pub(crate) fn mark_dirty(&mut self, key: NodeKey) {
        self.dirty.insert(key);
        if let Some(p) = self.nodes.get(&key).and_then(|n| n.parent) {
            self.dirty.insert(p);
        }
    }

    /// The key the next created node will get (always above every existing key).
    #[cfg(feature = "jetstream")]
    pub(crate) fn next_key(&self) -> u64 {
        self.next_key
    }

    /// Forget which nodes changed; for code that builds a whole document outside an update.
    pub fn clear_dirty(&mut self) {
        self.dirty.clear();
    }

    /// Replace the whole document with `other` (for example one an external tool produced),
    /// keeping this state's limits. Every old and new node counts as changed, so listeners,
    /// history and collaboration see it as an ordinary local edit.
    pub fn replace_document(&mut self, mut other: EditorState) {
        let mut dirty: BTreeSet<NodeKey> = self.nodes.keys().copied().collect();
        dirty.extend(other.nodes.keys().copied());
        other.limits = self.limits;
        other.dirty = dirty;
        other.validate_selection();
        *self = other;
    }

    /// Set the alignment and indent level of an element node.
    pub fn set_element_attrs(&mut self, key: NodeKey, align: Align, indent: u32) {
        let node = self.node_mut(key);
        node.align = align;
        node.indent = indent;
    }

    pub fn dirty_nodes(&self) -> impl Iterator<Item = NodeKey> + '_ {
        self.dirty.iter().copied().filter(|k| self.nodes.contains_key(k))
    }

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
}
