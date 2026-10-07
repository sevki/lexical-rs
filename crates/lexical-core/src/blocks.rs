//! Block-level operations: block types, lists, links, indentation, alignment.

use crate::error::Result;
use crate::format::Align;
use crate::node::*;
use crate::selection::Point;
use crate::state::EditorState;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockType {
    Paragraph,
    Heading(HeadingTag),
    Quote,
    Code,
}

impl BlockType {
    fn data(self) -> NodeData {
        match self {
            BlockType::Paragraph => NodeData::Paragraph,
            BlockType::Heading(t) => NodeData::Heading(t),
            BlockType::Quote => NodeData::Quote,
            BlockType::Code => NodeData::Code { language: None },
        }
    }
}

const MAX_INDENT: u32 = 10;
/// Deepest list nesting `Indent` will create (mirrored by `max_depth` in `verus/domains/editor.rs`).
pub const MAX_LIST_DEPTH: u32 = 8;

impl EditorState {
    /// Line blocks touched by the selection, in document order.
    pub fn selected_blocks(&self) -> Vec<NodeKey> {
        let Ok((s, e)) = self.ordered_points() else { return vec![] };
        let blocks = self.line_blocks();
        let find = |p: &Point| {
            self.line_block_of(self.inline_point(p).key)
                .and_then(|b| blocks.iter().position(|&x| x == b))
        };
        match (find(&s), find(&e)) {
            (Some(i), Some(j)) if i <= j => blocks[i..=j].to_vec(),
            _ => vec![],
        }
    }

    pub fn list_type_of_item(&self, item: NodeKey) -> Option<ListType> {
        let list = self.parent(item)?;
        match self.node(list).data {
            NodeData::List { list_type, .. } => Some(list_type),
            _ => None,
        }
    }

    /// Block type of the first selected block, for toolbar state.
    pub fn current_block(&self) -> Option<NodeKey> {
        self.selected_blocks().first().copied()
    }

    // ------------------------------------------------------------ block type

    pub fn set_block_type(&mut self, ty: BlockType) -> Result<()> {
        for b in self.selected_blocks() {
            let mut b = b;
            if self.node(b).node_type() == NodeType::ListItem {
                b = self.unlist_item(b);
            }
            if self.node(b).data == ty.data() {
                continue;
            }
            let (indent, align) = (self.node(b).indent, self.node(b).align);
            let n = self.create_node(ty.data());
            self.node_mut(n).indent = indent;
            self.node_mut(n).align = align;
            self.replace_element(b, n);
        }
        Ok(())
    }

    pub fn set_align(&mut self, align: Align) -> Result<()> {
        for b in self.selected_blocks() {
            self.node_mut(b).align = align;
        }
        Ok(())
    }

    // ----------------------------------------------------------------- lists

    /// Turn the selected blocks into a list of `ty`, or remove the list when
    /// every selected block already is one.
    pub fn toggle_list(&mut self, ty: ListType) -> Result<()> {
        let blocks = self.selected_blocks();
        if blocks.is_empty() {
            return Ok(());
        }
        let all_in = blocks.iter().all(|&b| self.list_type_of_item(b) == Some(ty));
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
            let list = self.create_node(NodeData::List { list_type: ty, start: 1 });
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
        let Some(list) = self.parent(item) else { return };
        let NodeData::List { list_type, .. } = self.node(list).data else { return };
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
        let mid = self.create_node(NodeData::List { list_type: ty, start: 1 });
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
                *checked = if ty == ListType::Check { checked.or(Some(false)) } else { None };
            }
        }
    }

    pub fn merge_adjacent_lists(&mut self) {
        loop {
            let mut merged = false;
            let all: Vec<NodeKey> =
                self.nodes.keys().copied().filter(|&k| self.node(k).node_type() == NodeType::List).collect();
            for l in all {
                if !self.contains(l) {
                    continue;
                }
                let Some(next) = self.next_sibling(l) else { continue };
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
        while let Some(wrapper) = self
            .parent(item)
            .and_then(|nl| self.parent(nl))
            .filter(|&w| self.is_wrapper(w))
        {
            let _ = wrapper;
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
                Some(p) if matches!(self.node(p).node_type(), NodeType::ListItem | NodeType::List) => cur = p,
                _ => return,
            }
        }
    }

    pub fn toggle_check(&mut self, item: NodeKey) {
        if self.list_type_of_item(item) == Some(ListType::Check)
            && let NodeData::ListItem { checked } = &mut self.node_mut(item).data {
                *checked = Some(!checked.unwrap_or(false));
            }
    }

    // ---------------------------------------------------------- indentation

    pub fn indent_blocks(&mut self) -> Result<()> {
        for b in self.selected_blocks() {
            if self.node(b).node_type() == NodeType::ListItem {
                self.indent_item(b);
            } else if self.node(b).indent < MAX_INDENT {
                self.node_mut(b).indent += 1;
            }
        }
        Ok(())
    }

    pub fn outdent_blocks(&mut self) -> Result<()> {
        for b in self.selected_blocks() {
            if self.node(b).node_type() == NodeType::ListItem {
                self.outdent_item(b);
            } else if self.node(b).indent > 0 {
                self.node_mut(b).indent -= 1;
            }
        }
        Ok(())
    }

    fn is_wrapper(&self, item: NodeKey) -> bool {
        self.node(item).node_type() == NodeType::ListItem
            && self.node(item).children.iter().any(|&c| self.node(c).node_type() == NodeType::List)
    }

    /// Nesting depth of a list item: 0 for a top-level item.
    pub fn list_depth(&self, item: NodeKey) -> u32 {
        let lists = self.ancestors(item).iter().filter(|&&a| self.node(a).node_type() == NodeType::List).count();
        lists.saturating_sub(1) as u32
    }

    fn indent_item(&mut self, item: NodeKey) {
        let Some(list) = self.parent(item) else { return };
        if self.list_depth(item) >= MAX_LIST_DEPTH {
            return;
        }
        let data = self.node(list).data.clone();
        let (prev, next) = (self.prev_sibling(item), self.next_sibling(item));
        // A wrapper may hold several lists (after a type change); join the adjacent one.
        let lists_of = |s: &EditorState, w: NodeKey| -> Vec<NodeKey> {
            s.node(w).children.iter().copied().filter(|&c| s.node(c).node_type() == NodeType::List).collect()
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

    fn outdent_item(&mut self, item: NodeKey) {
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

    // ----------------------------------------------------------------- links

    /// Wrap the selection in a link (`Some(url)`), or remove links (`None`).
    pub fn toggle_link(&mut self, url: Option<&str>) -> Result<()> {
        let sel = self.require_selection()?;
        if sel.is_collapsed() {
            if url.is_none() {
                let p = self.inline_point(&sel.anchor);
                if let Some(l) = self.ancestors(p.key).into_iter().find(|&a| self.node(a).is_inline()) {
                    self.unwrap_link(l);
                }
            }
            return Ok(());
        }
        let backward = self.is_backward();
        let (s, e) = self.ordered_points()?;
        let (a, b) = self.split_range(&s, &e);
        let leaves = self.leaves_between(&a, &b);
        if leaves.is_empty() {
            return Ok(());
        }
        let (first, last) = (leaves[0], *leaves.last().unwrap());
        // Process each run of adjacent siblings. A run inside an existing link is first
        // isolated into its own link, so text outside the selection keeps its old link.
        let mut i = 0;
        while i < leaves.len() {
            let parent = self.parent(leaves[i]).unwrap();
            let mut j = i;
            while j + 1 < leaves.len() && self.next_sibling(leaves[j]) == Some(leaves[j + 1]) {
                j += 1;
            }
            if self.node(parent).is_inline() {
                let link = self.isolate_in_link(parent, leaves[i], leaves[j]);
                match url {
                    Some(u) => {
                        if let NodeData::Link { url: lu, .. } = &mut self.node_mut(link).data {
                            *lu = u.to_string();
                        }
                    }
                    None => self.unwrap_link(link),
                }
            } else if let Some(u) = url {
                let link = self.create_node(NodeData::Link {
                    url: u.to_string(),
                    target: None,
                    rel: None,
                    title: None,
                });
                self.insert_before(leaves[i], link);
                for &l in &leaves[i..=j] {
                    self.append_child(link, l);
                }
            }
            i = j + 1;
        }
        let leaf_pt = |s: &EditorState, k: NodeKey, end: bool| {
            if s.node(k).is_text() {
                Point::text(k, if end { s.node(k).text_len() } else { 0 })
            } else {
                let p = s.parent(k).unwrap();
                Point::element(p, s.index_in_parent(k).unwrap() + end as usize)
            }
        };
        let (sp, ep) = (leaf_pt(self, first, false), leaf_pt(self, last, true));
        let sel = self.selection.as_mut().unwrap();
        if backward {
            sel.anchor = ep;
            sel.focus = sp;
        } else {
            sel.anchor = sp;
            sel.focus = ep;
        }
        Ok(())
    }

    /// Split `link` so that a link containing exactly the children `first..=last`
    /// exists (copies of its attributes keep the head and tail linked as before).
    /// Returns that link.
    fn isolate_in_link(&mut self, link: NodeKey, first: NodeKey, last: NodeKey) -> NodeKey {
        let data = self.node(link).data.clone();
        let mut target = link;
        let a = self.index_in_parent(first).unwrap();
        if a > 0 {
            let tail: Vec<_> = self.node(link).children[a..].to_vec();
            target = self.create_node(data.clone());
            self.insert_after(link, target);
            for k in tail {
                self.append_child(target, k);
            }
        }
        let b = self.index_in_parent(last).unwrap();
        if b + 1 < self.node(target).children.len() {
            let tail: Vec<_> = self.node(target).children[b + 1..].to_vec();
            let rest = self.create_node(data);
            self.insert_after(target, rest);
            for k in tail {
                self.append_child(rest, k);
            }
        }
        target
    }

    fn unwrap_link(&mut self, link: NodeKey) {
        let kids = self.node(link).children.clone();
        for k in kids {
            self.insert_before(link, k);
        }
        self.remove(link);
    }

    /// URL of the link containing the selection anchor, if any.
    pub fn link_at_selection(&self) -> Option<String> {
        let sel = self.selection.as_ref()?;
        let p = self.inline_point(&sel.anchor);
        std::iter::once(p.key).chain(self.ancestors(p.key)).find_map(|k| match &self.get(k)?.data {
            NodeData::Link { url, .. } => Some(url.clone()),
            _ => None,
        })
    }
}
