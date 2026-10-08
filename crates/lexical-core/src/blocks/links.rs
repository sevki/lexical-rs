//! Links: wrapping a selection, re-targeting part of a link, and removing links.

use crate::error::Result;
use crate::node::*;
use crate::selection::Point;
use crate::state::EditorState;

impl EditorState {
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
