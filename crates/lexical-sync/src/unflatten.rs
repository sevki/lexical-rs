//! Rebuild a Lexical document from flat text.
//!
//! This is total: *any* [`Flat`] (including the odd combinations a CRDT merge can
//! produce, such as a missing final terminator or an item two levels deeper than its
//! predecessor) yields a document that satisfies `EditorState::check_invariants`.

use crate::flat::{BlockKind, CharAttr, Flat, Inline, Line};
use lexical_core::{
    EditorState, ListType, NodeData, NodeKey, NodeType, Point, Selection, TextFormat, TextMode, ROOT_KEY,
};

pub fn unflatten(flat: &Flat) -> EditorState {
    let mut state = EditorState::empty();
    let mut lists = ListStack::default();
    let mut start = 0;
    for end in 0..flat.len() {
        if let CharAttr::End(line) = &flat.attrs[end] {
            build_line(&mut state, &mut lists, flat, start..end, line);
            start = end + 1;
        }
    }
    if start < flat.len() {
        // Text after the last terminator: a final line with default attributes.
        build_line(&mut state, &mut lists, flat, start..flat.len(), &Line::default());
    }
    if state.root_children().is_empty() {
        let p = state.create_paragraph();
        state.append_child(ROOT_KEY, p);
    }
    let first = state.line_blocks()[0];
    state.selection = Some(Selection::collapsed(Point::element(first, 0)));
    state.clear_dirty();
    state
}

fn build_line(
    state: &mut EditorState,
    lists: &mut ListStack,
    flat: &Flat,
    range: std::ops::Range<usize>,
    line: &Line,
) {
    let block = match line.kind {
        BlockKind::ListItem(ty) => {
            let list = lists.list_for(state, line.depth as usize, ty);
            let checked = (ty == ListType::Check).then(|| line.checked.unwrap_or(false));
            let item = state.create_node(NodeData::ListItem { checked });
            state.append_child(list, item);
            item
        }
        other => {
            lists.clear();
            let data = match other {
                BlockKind::Heading(t) => NodeData::Heading(t),
                BlockKind::Quote => NodeData::Quote,
                BlockKind::Code => NodeData::Code { language: None },
                _ => NodeData::Paragraph,
            };
            let node = state.create_node(data);
            state.append_child(ROOT_KEY, node);
            node
        }
    };
    state.set_element_attrs(block, line.align, line.indent);
    fill_inline(state, block, flat, range);
}

/// Turn a run of characters into text, link and line-break children of `parent`.
fn fill_inline(state: &mut EditorState, parent: NodeKey, flat: &Flat, range: std::ops::Range<usize>) {
    let mut link: Option<(String, NodeKey)> = None;
    let mut i = range.start;
    while i < range.end {
        match &flat.attrs[i] {
            CharAttr::Soft => {
                let lb = state.create_node(NodeData::LineBreak);
                state.append_child(parent, lb);
                link = None;
                i += 1;
            }
            CharAttr::End(_) => i += 1,
            CharAttr::Inline(inline) => {
                let mut j = i + 1;
                while j < range.end && flat.attrs[j] == flat.attrs[i] {
                    j += 1;
                }
                let node = text_node(state, &flat.chars[i..j], inline);
                match &inline.link {
                    Some(url) => {
                        let container = match &link {
                            Some((u, k)) if u == url => *k,
                            _ => {
                                let k = state.create_node(NodeData::Link {
                                    url: url.clone(),
                                    target: None,
                                    rel: None,
                                    title: None,
                                });
                                state.append_child(parent, k);
                                link = Some((url.clone(), k));
                                k
                            }
                        };
                        state.append_child(container, node);
                    }
                    None => {
                        link = None;
                        state.append_child(parent, node);
                    }
                }
                i = j;
            }
        }
    }
}

fn text_node(state: &mut EditorState, chars: &[char], inline: &Inline) -> NodeKey {
    state.create_node(NodeData::Text {
        text: chars.iter().collect(),
        format: inline.format & TextFormat::all(),
        style: inline.style.clone(),
        mode: TextMode::Normal,
        detail: 0,
    })
}

/// The chain of open lists while lines are appended, one per nesting depth.
#[derive(Default)]
struct ListStack {
    lists: Vec<(NodeKey, ListType)>,
}

impl ListStack {
    fn clear(&mut self) {
        self.lists.clear();
    }

    /// The list a new item at `depth` belongs in, creating wrapper items and lists as
    /// needed (a type change at the same depth starts a new list).
    fn list_for(&mut self, state: &mut EditorState, depth: usize, ty: ListType) -> NodeKey {
        self.lists.truncate(depth + 1);
        if self.lists.len() == depth + 1 && self.lists[depth].1 != ty {
            self.lists.pop();
        }
        while self.lists.len() < depth + 1 {
            let level = self.lists.len();
            let list_type = if level == depth { ty } else { self.lists.last().map_or(ty, |l| l.1) };
            let list = state.create_node(NodeData::List { list_type, start: 1 });
            match self.lists.last() {
                None => state.append_child(ROOT_KEY, list),
                Some(&(parent_list, _)) => {
                    // Reuse a trailing wrapper (a nested list of another type just ended
                    // there): adjacent wrappers are one wrapper holding several lists.
                    let trailing = state.children(parent_list).last().copied().filter(|&c| {
                        state.node(c).children.iter().any(|&g| state.node(g).node_type() == NodeType::List)
                    });
                    let wrapper = trailing.unwrap_or_else(|| {
                        let w = state.create_node(NodeData::ListItem { checked: None });
                        state.append_child(parent_list, w);
                        w
                    });
                    state.append_child(wrapper, list);
                }
            }
            self.lists.push((list, list_type));
        }
        self.lists[depth].0
    }
}
