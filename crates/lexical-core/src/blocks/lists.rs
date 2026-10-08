//! Lists: toggling, per-item type changes, nesting, and leaving a list.
//!
//! A nested list lives inside a "wrapper" list item whose only children are lists
//! (Lexical's own representation), so an item at depth N sits under N wrappers.

use crate::error::Result;
use crate::node::*;
use crate::state::EditorState;

impl EditorState {
    pub fn list_type_of_item(&self, item: NodeKey) -> Option<ListType> {
        let list = self.parent(item)?;
        match self.node(list).data {
            NodeData::List { list_type, .. } => Some(list_type),
            _ => None,
        }
    }

    /// Turn the selected blocks into a list of `ty`, or remove the list when
    /// every selected block already is one.
    pub fn toggle_list(&mut self, ty: ListType) -> Result<()> {
        let blocks = self.selected_blocks();
        if blocks.is_empty() {
            return Ok(());
        }
        let all_in = blocks
            .iter()
            .all(|&b| self.list_type_of_item(b) == Some(ty));
        if all_in {
            for b in blocks {
                self.unlist_item(b);
            }
            return Ok(());
        }
        for b in blocks {
            if self.node(b).node_type() == NodeType::ListItem {
                self.retype_item(b, ty);
                continue;
            }
            let item = self.create_node(NodeData::ListItem {
                checked: (ty == ListType::Check).then_some(false),
            });
            let align = self.node(b).align;
            let list = self.create_node(NodeData::List {
                list_type: ty,
                start: 1,
            });
            self.node_mut(item).align = align;
            self.insert_after(b, list);
            self.append_child(list, item);
            self.transfer_children(b, item);
            self.remove(b);
        }
        self.merge_adjacent_lists();
        Ok(())
    }

    /// Give `item` list type `ty` without touching its siblings: the list is split
    /// around the item (head / item / tail) unless the item is alone in it.
    fn retype_item(&mut self, item: NodeKey, ty: ListType) {
        let Some(list) = self.parent(item) else {
            return;
        };
        let NodeData::List { list_type, .. } = self.node(list).data else {
            return;
        };
        if list_type == ty {
            return;
        }
        let idx = self.index_in_parent(item).unwrap();
        if self.node(list).children.len() == 1 {
            if let NodeData::List { list_type, .. } = &mut self.node_mut(list).data {
                *list_type = ty;
            }
            self.fix_checked(list);
            return;
        }
        let tail: Vec<_> = self.node(list).children[idx + 1..].to_vec();
        let tail_data = self.node(list).data.clone();
        let mid = self.create_node(NodeData::List {
            list_type: ty,
            start: 1,
        });
        self.insert_after(list, mid);
        self.append_child(mid, item);
        if !tail.is_empty() {
            let rest = self.create_node(tail_data);
            self.insert_after(mid, rest);
            for t in tail {
                self.append_child(rest, t);
            }
        }
        self.fix_checked(mid);
        self.prune_empty_lists(list);
    }

    fn fix_checked(&mut self, list: NodeKey) {
        let ty = match self.node(list).data {
            NodeData::List { list_type, .. } => list_type,
            _ => return,
        };
        for it in self.node(list).children.clone() {
            if let NodeData::ListItem { checked } = &mut self.node_mut(it).data {
                *checked = if ty == ListType::Check {
                    checked.or(Some(false))
                } else {
                    None
                };
            }
        }
    }

    pub fn merge_adjacent_lists(&mut self) {
        loop {
            let mut merged = false;
            let all: Vec<NodeKey> = self
                .nodes
                .keys()
                .copied()
                .filter(|&k| self.node(k).node_type() == NodeType::List)
                .collect();
            for l in all {
                if !self.contains(l) {
                    continue;
                }
                let Some(next) = self.next_sibling(l) else {
                    continue;
                };
                if self.node(next).node_type() != NodeType::List {
                    continue;
                }
                let (NodeData::List { list_type: a, .. }, NodeData::List { list_type: b, .. }) =
                    (&self.node(l).data, &self.node(next).data)
                else {
                    continue;
                };
                if a == b {
                    for k in self.node(next).children.clone() {
                        self.append_child(l, k);
                    }
                    self.remove(next);
                    merged = true;
                    break;
                }
            }
            if !merged {
                break;
            }
        }
    }

    /// Convert a list item into a paragraph placed after (a split of) its list.
    /// Returns the new paragraph.
    pub fn unlist_item(&mut self, item: NodeKey) -> NodeKey {
        // A nested item first climbs out to the top-level list, otherwise the
        // paragraph would end up inside a wrapper list item.
        while self
            .parent(item)
            .and_then(|nl| self.parent(nl))
            .is_some_and(|w| self.is_wrapper(w))
        {
            self.outdent_item(item);
        }
        let list = self.parent(item).expect("list item without list");
        let idx = self.index_in_parent(item).unwrap();
        let para = self.create_node(NodeData::Paragraph);
        self.node_mut(para).align = self.node(item).align;
        // Items after this one move into a fresh list placed behind the paragraph.
        let tail: Vec<_> = self.node(list).children[idx + 1..].to_vec();
        self.insert_after(list, para);
        if !tail.is_empty() {
            let data = self.node(list).data.clone();
            let nl = self.create_node(data);
            self.insert_after(para, nl);
            for t in tail {
                self.append_child(nl, t);
            }
        }
        self.transfer_children(item, para);
        self.remove(item);
        self.prune_empty_lists(list);
        para
    }

    fn prune_empty_lists(&mut self, mut cur: NodeKey) {
        loop {
            if !self.contains(cur) || cur == ROOT_KEY || !self.node(cur).children.is_empty() {
                return;
            }
            let parent = self.parent(cur);
            self.remove(cur);
            match parent {
                Some(p)
                    if matches!(
                        self.node(p).node_type(),
                        NodeType::ListItem | NodeType::List
                    ) =>
                {
                    cur = p
                }
                _ => return,
            }
        }
    }

    pub fn toggle_check(&mut self, item: NodeKey) {
        if self.list_type_of_item(item) == Some(ListType::Check)
            && let NodeData::ListItem { checked } = &mut self.node_mut(item).data
        {
            *checked = Some(!checked.unwrap_or(false));
        }
    }

    pub(super) fn is_wrapper(&self, item: NodeKey) -> bool {
        self.node(item).node_type() == NodeType::ListItem
            && self
                .node(item)
                .children
                .iter()
                .any(|&c| self.node(c).node_type() == NodeType::List)
    }

    /// Nesting depth of a list item: 0 for a top-level item.
    pub fn list_depth(&self, item: NodeKey) -> u32 {
        let lists = self
            .ancestors(item)
            .iter()
            .filter(|&&a| self.node(a).node_type() == NodeType::List)
            .count();
        lists.saturating_sub(1) as u32
    }

    pub(super) fn indent_item(&mut self, item: NodeKey) {
        let Some(list) = self.parent(item) else {
            return;
        };
        if self
            .limits
            .max_list_depth
            .is_some_and(|max| self.list_depth(item) >= max)
        {
            return;
        }
        let data = self.node(list).data.clone();
        let (prev, next) = (self.prev_sibling(item), self.next_sibling(item));
        // A wrapper may hold several lists (after a type change); join the adjacent one.
        let lists_of = |s: &EditorState, w: NodeKey| -> Vec<NodeKey> {
            s.node(w)
                .children
                .iter()
                .copied()
                .filter(|&c| s.node(c).node_type() == NodeType::List)
                .collect()
        };
        if let Some(w) = prev.filter(|&w| self.is_wrapper(w)) {
            let nl = *lists_of(self, w).last().unwrap();
            self.append_child(nl, item);
        } else if let Some(w) = next.filter(|&w| self.is_wrapper(w)) {
            let nl = lists_of(self, w)[0];
            self.insert_child(nl, 0, item);
        } else {
            let w = self.create_node(NodeData::ListItem { checked: None });
            let nl = self.create_node(data);
            self.insert_before(item, w);
            self.append_child(w, nl);
            self.append_child(nl, item);
        }
    }

    pub(super) fn outdent_item(&mut self, item: NodeKey) {
        let Some(nl) = self.parent(item) else { return };
        let wrapper = self.parent(nl).filter(|&w| self.is_wrapper(w));
        let Some(w) = wrapper else {
            self.unlist_item(item);
            return;
        };
        let outer = self.parent(w).unwrap();
        let idx = self.index_in_parent(item).unwrap();
        let tail: Vec<_> = self.node(nl).children[idx + 1..].to_vec();
        let wi = self.index_in_parent(w).unwrap();
        // Item moves out right after the wrapper; the tail forms a new wrapper after it.
        self.insert_child(outer, wi + 1, item);
        if !tail.is_empty() {
            let nw = self.create_node(NodeData::ListItem { checked: None });
            let data = self.node(nl).data.clone();
            let nnl = self.create_node(data);
            self.insert_child(outer, wi + 2, nw);
            self.append_child(nw, nnl);
            for t in tail {
                self.append_child(nnl, t);
            }
        }
        self.prune_empty_lists(nl);
    }
}
