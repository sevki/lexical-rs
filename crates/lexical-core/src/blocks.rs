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
                if let Some(list) = self.parent(b) {
                    if let NodeData::List { list_type, .. } = &mut self.node_mut(list).data {
                        *list_type = ty;
                    }
                    self.fix_checked(list);
                }
                continue;
            }
            let item = self.create_node(NodeData::ListItem {
                checked: (ty == ListType::Check).then_some(false),
            });
            let align = self.node(b).align;
            let list = self.create_node(NodeData::List { list_type: ty, start: 1 });
            self.node_mut(item).align = align;
            self.insert_after(b, list);
            // Move content across by hand: `replace_element` would re-parent `item`.
            for k in self.node(b).children.clone() {
                self.append_child(item, k);
            }
            self.append_child(list, item);
            if let Some(sel) = self.selection.as_mut() {
                for p in [&mut sel.anchor, &mut sel.focus] {
                    if p.key == b {
                        p.key = item;
                    }
                }
            }
            self.remove(b);
        }
        self.merge_adjacent_lists();
        Ok(())
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
        self.replace_element(item, para);
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
        if self.list_type_of_item(item) == Some(ListType::Check) {
            if let NodeData::ListItem { checked } = &mut self.node_mut(item).data {
                *checked = Some(!checked.unwrap_or(false));
            }
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

    fn indent_item(&mut self, item: NodeKey) {
        let Some(list) = self.parent(item) else { return };
        let data = self.node(list).data.clone();
        let (prev, next) = (self.prev_sibling(item), self.next_sibling(item));
        let nested = |s: &EditorState, w: NodeKey| s.node(w).children.iter().copied().find(|&c| s.node(c).node_type() == NodeType::List);
        if let Some(w) = prev.filter(|&w| self.is_wrapper(w)) {
            let nl = nested(self, w).unwrap();
            self.append_child(nl, item);
        } else if let Some(w) = next.filter(|&w| self.is_wrapper(w)) {
            let nl = nested(self, w).unwrap();
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
        match url {
            None => {
                let mut links: Vec<NodeKey> = vec![];
                for &l in &leaves {
                    if let Some(p) = self.parent(l).filter(|&p| self.node(p).is_inline()) {
                        if !links.contains(&p) {
                            links.push(p);
                        }
                    }
                }
                for l in links {
                    self.unwrap_link(l);
                }
            }
            Some(u) => {
                let mut i = 0;
                while i < leaves.len() {
                    let parent = self.parent(leaves[i]).unwrap();
                    let mut j = i;
                    while j + 1 < leaves.len() && self.next_sibling(leaves[j]) == Some(leaves[j + 1]) {
                        j += 1;
                    }
                    if let NodeData::Link { url: lu, .. } = &mut self.node_mut(parent).data {
                        *lu = u.to_string();
                    } else {
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
            }
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
