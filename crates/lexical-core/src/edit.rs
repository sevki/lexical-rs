//! Selection-driven editing: typing, deleting, paragraphs, line breaks, formatting.

use crate::error::Result;
use crate::format::TextFormat;
use crate::node::*;
use crate::selection::{Point, PointKind, Selection};
use crate::state::EditorState;
use unicode_segmentation::UnicodeSegmentation;

/// A caret position between two children of `parent`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Boundary {
    pub parent: NodeKey,
    pub before: Option<NodeKey>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Granularity {
    Character,
    Word,
    Line,
}

impl EditorState {
    pub(crate) fn boundary_index(&self, b: &Boundary) -> usize {
        match b.before {
            Some(k) => self.index_in_parent(k).expect("boundary node detached"),
            None => self.node(b.parent).children.len(),
        }
    }

    /// Turn a point into a boundary, splitting a text node when the point is inside it.
    pub(crate) fn boundary_from_point(&mut self, p: &Point) -> Boundary {
        let p = self.inline_point(p);
        match p.kind {
            PointKind::Element => {
                let kids = &self.node(p.key).children;
                Boundary { parent: p.key, before: kids.get(p.offset).copied() }
            }
            PointKind::Text => {
                let len = self.node(p.key).text_len();
                let parent = self.parent(p.key).expect("text without parent");
                if p.offset == 0 {
                    Boundary { parent, before: Some(p.key) }
                } else if p.offset >= len {
                    Boundary { parent, before: self.next_sibling(p.key) }
                } else {
                    let pieces = self.split_text(p.key, &[p.offset]);
                    Boundary { parent, before: Some(pieces[1]) }
                }
            }
        }
    }

    /// Split the range's text nodes at both ends; returns the two boundaries.
    pub(crate) fn split_range(&mut self, start: &Point, end: &Point) -> (Boundary, Boundary) {
        let b = self.boundary_from_point(end);
        let a = self.boundary_from_point(start);
        (a, b)
    }

    fn boundary_pos(&self, b: &Boundary, pre: &std::collections::HashMap<NodeKey, usize>, end: &std::collections::HashMap<NodeKey, usize>) -> usize {
        match b.before {
            Some(k) => pre[&k],
            None => end[&b.parent],
        }
    }

    /// Leaves (text/linebreak) between two boundaries, in document order.
    pub(crate) fn leaves_between(&self, a: &Boundary, b: &Boundary) -> Vec<NodeKey> {
        let (pre, end) = self.preorder();
        let (pa, pb) = (self.boundary_pos(a, &pre, &end), self.boundary_pos(b, &pre, &end));
        self.leaves().into_iter().filter(|k| pre[k] >= pa && pre[k] < pb).collect()
    }

    // ----------------------------------------------------------------- text

    pub fn insert_text(&mut self, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        if text.contains('\n') || text.contains('\r') {
            return self.insert_raw_text(text);
        }
        let sel = self.require_selection()?;
        if !sel.is_collapsed() {
            self.remove_text()?;
        }
        let sel = self.require_selection()?;
        let (format, style) = (sel.format, sel.style.clone());
        let p = self.inline_point(&sel.anchor);
        let (mut key, mut offset) = match p.kind {
            PointKind::Text => (p.key, p.offset),
            PointKind::Element => {
                let kids = self.node(p.key).children.clone();
                let prev = p.offset.checked_sub(1).and_then(|i| kids.get(i)).copied();
                let next = kids.get(p.offset).copied();
                if let Some(pk) = prev.filter(|&k| self.node(k).is_text()) {
                    (pk, self.node(pk).text_len())
                } else if let Some(nk) = next.filter(|&k| self.node(k).is_text()) {
                    (nk, 0)
                } else {
                    let n = self.create_node(NodeData::Text {
                        text: String::new(),
                        format,
                        style: style.clone(),
                        mode: TextMode::Normal,
                        detail: 0,
                    });
                    self.insert_child(p.key, p.offset, n);
                    (n, 0)
                }
            }
        };
        let node = self.node(key);
        if node.text_format() != format || node.text_style() != style {
            // Typing with different pending formatting: new node at the caret.
            let len = node.text_len();
            let fresh = self.create_node(NodeData::Text {
                text: String::new(),
                format,
                style,
                mode: TextMode::Normal,
                detail: 0,
            });
            if offset == 0 {
                self.insert_before(key, fresh);
            } else if offset >= len {
                self.insert_after(key, fresh);
            } else {
                let pieces = self.split_text(key, &[offset]);
                self.insert_after(pieces[0], fresh);
            }
            key = fresh;
            offset = 0;
        }
        let old = self.node(key).text().unwrap().to_string();
        let at = byte_index(&old, offset);
        let mut new = String::with_capacity(old.len() + text.len());
        new.push_str(&old[..at]);
        new.push_str(text);
        new.push_str(&old[at..]);
        self.set_text(key, &new);
        let sel = self.selection.as_mut().unwrap();
        sel.anchor = Point::text(key, offset + text.chars().count());
        sel.focus = sel.anchor;
        Ok(())
    }

    /// Insert text that may contain newlines (paste). Newlines become paragraphs,
    /// or line breaks inside code blocks.
    pub fn insert_raw_text(&mut self, text: &str) -> Result<()> {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let mut first = true;
        for line in normalized.split('\n') {
            if !first {
                let in_code = self.require_selection().ok().is_some_and(|s| {
                    self.line_block_of(s.anchor.key)
                        .is_some_and(|b| self.node(b).node_type() == NodeType::Code)
                });
                if in_code {
                    self.insert_line_break()?;
                } else {
                    self.insert_paragraph()?;
                }
            }
            first = false;
            if !line.is_empty() {
                self.insert_text(line)?;
            }
        }
        Ok(())
    }

    pub fn insert_line_break(&mut self) -> Result<()> {
        let sel = self.require_selection()?;
        if !sel.is_collapsed() {
            self.remove_text()?;
        }
        let sel = self.require_selection()?;
        let b = self.boundary_from_point(&sel.anchor);
        let idx = self.boundary_index(&b);
        let lb = self.create_node(NodeData::LineBreak);
        self.insert_child(b.parent, idx, lb);
        self.selection.as_mut().unwrap().anchor = Point::element(b.parent, idx + 1);
        self.selection.as_mut().unwrap().focus = Point::element(b.parent, idx + 1);
        Ok(())
    }

    // --------------------------------------------------------------- delete

    /// Delete the selected range (no-op when collapsed).
    pub fn remove_text(&mut self) -> Result<()> {
        let sel = self.require_selection()?;
        if sel.is_collapsed() {
            return Ok(());
        }
        let (start, end) = self.ordered_points()?;
        let (a, b) = self.split_range(&start, &end);
        let idx_a = self.boundary_index(&a);
        self.delete_between(&a, &b);
        let caret = self.prefer_text_point(a.parent, idx_a);
        self.set_caret(caret);
        Ok(())
    }

    fn prefer_text_point(&self, parent: NodeKey, idx: usize) -> Point {
        let kids = &self.node(parent).children;
        let idx = idx.min(kids.len());
        if let Some(&p) = idx.checked_sub(1).and_then(|i| kids.get(i)) {
            if self.node(p).is_text() {
                return Point::text(p, self.node(p).text_len());
            }
        }
        if let Some(&n) = kids.get(idx) {
            if self.node(n).is_text() {
                return Point::text(n, 0);
            }
        }
        Point::element(parent, idx)
    }

    pub(crate) fn delete_between(&mut self, a: &Boundary, b: &Boundary) {
        let ia = self.boundary_index(a);
        let ib = self.boundary_index(b);
        if a.parent == b.parent {
            let kids: Vec<_> = self.node(a.parent).children[ia..ib.max(ia)].to_vec();
            for k in kids {
                self.remove(k);
            }
            return;
        }
        let chain = |s: &EditorState, from: NodeKey| -> Vec<NodeKey> {
            let mut v = vec![from];
            v.extend(s.ancestors(from));
            v
        };
        let (ca, cb) = (chain(self, a.parent), chain(self, b.parent));
        let common = *ca.iter().find(|k| cb.contains(k)).expect("no common ancestor");
        let pa = ca.iter().position(|&k| k == common).unwrap();
        let pb = cb.iter().position(|&k| k == common).unwrap();

        let mut doomed: Vec<NodeKey> = vec![];
        // Middle: between the two paths inside `common`.
        let lo = if pa == 0 { ia } else { self.index_in_parent(ca[pa - 1]).unwrap() + 1 };
        let hi = if pb == 0 { ib } else { self.index_in_parent(cb[pb - 1]).unwrap() };
        if hi > lo {
            doomed.extend_from_slice(&self.node(common).children[lo..hi]);
        }
        // Left tail.
        for k in 0..pa {
            let from = if k == 0 { ia } else { self.index_in_parent(ca[k - 1]).unwrap() + 1 };
            doomed.extend_from_slice(&self.node(ca[k]).children[from..]);
        }
        // Right head.
        for k in 0..pb {
            let to = if k == 0 { ib } else { self.index_in_parent(cb[k - 1]).unwrap() };
            doomed.extend_from_slice(&self.node(cb[k]).children[..to]);
        }
        for k in doomed {
            self.remove(k);
        }
        // Merge the end block into the start block.
        let blk_a = self.line_block_of(a.parent);
        let blk_b = self.line_block_of(b.parent);
        if let (Some(ba), Some(bb)) = (blk_a, blk_b) {
            if ba != bb {
                let old_parent = self.parent(bb);
                let kids = self.node(bb).children.clone();
                for k in kids {
                    self.append_child(ba, k);
                }
                self.remove(bb);
                let mut cur = old_parent;
                while let Some(c) = cur {
                    if c == ROOT_KEY || !self.node(c).children.is_empty() {
                        break;
                    }
                    cur = self.parent(c);
                    self.remove(c);
                }
            }
        }
    }

    pub fn delete_character(&mut self, backward: bool) -> Result<()> {
        self.delete_by(backward, Granularity::Character)
    }

    pub fn delete_word(&mut self, backward: bool) -> Result<()> {
        self.delete_by(backward, Granularity::Word)
    }

    pub fn delete_line(&mut self, backward: bool) -> Result<()> {
        self.delete_by(backward, Granularity::Line)
    }

    fn delete_by(&mut self, backward: bool, gran: Granularity) -> Result<()> {
        let sel = self.require_selection()?;
        if !sel.is_collapsed() {
            return self.remove_text();
        }
        let pt = self.inline_point(&sel.anchor);
        let Some(block) = self.line_block_of(pt.key) else { return Ok(()) };
        let content = self.block_content(block);
        let off = self.block_offset(&content, &pt);

        if backward && off == 0 {
            match self.node(block).data {
                NodeData::ListItem { .. } => return self.outdent_blocks(),
                NodeData::Heading(_) | NodeData::Quote | NodeData::Code { .. } => {
                    return self.set_block_type(crate::blocks::BlockType::Paragraph)
                }
                NodeData::Paragraph if self.node(block).indent > 0 => return self.outdent_blocks(),
                _ => {}
            }
        }
        if (backward && off == 0) || (!backward && off == content.len) {
            let blocks = self.line_blocks();
            let i = blocks.iter().position(|&b| b == block).unwrap();
            let other = if backward { i.checked_sub(1) } else { Some(i + 1) }
                .and_then(|j| blocks.get(j).copied());
            let Some(other) = other else { return Ok(()) };
            let (a, f) = if backward {
                (Point::element(other, self.node(other).children.len()), pt)
            } else {
                (pt, Point::element(other, 0))
            };
            self.selection = Some(Selection { anchor: a, focus: f, ..sel });
            return self.remove_text();
        }
        let target = step_offset(&content.text, off, backward, gran);
        let tp = self.block_point(block, &content, target);
        self.selection = Some(Selection { anchor: pt, focus: tp, ..sel });
        self.remove_text()
    }

    // ------------------------------------------------------------ paragraphs

    pub fn insert_paragraph(&mut self) -> Result<()> {
        let sel = self.require_selection()?;
        if !sel.is_collapsed() {
            self.remove_text()?;
        }
        let sel = self.require_selection()?;
        let pending = sel.format;
        let p = self.inline_point(&sel.anchor);
        let Some(block) = self.line_block_of(p.key) else {
            // Empty root: create a paragraph.
            let np = self.create_paragraph();
            self.append_child(ROOT_KEY, np);
            self.set_caret(Point::element(np, 0));
            return Ok(());
        };
        let content = self.block_content(block);
        let off = self.block_offset(&content, &p);
        let at_end = off == content.len;
        let node_type = self.node(block).node_type();

        if node_type == NodeType::Code {
            let ends_with_break = self
                .node(block)
                .children
                .last()
                .is_some_and(|&k| self.node(k).is_linebreak());
            if at_end && ends_with_break {
                // Double enter at the end exits the code block.
                let last = *self.node(block).children.last().unwrap();
                self.remove(last);
                let np = self.create_paragraph();
                self.insert_after(block, np);
                self.set_caret(Point::element(np, 0));
                return Ok(());
            }
            return self.insert_line_break();
        }
        if node_type == NodeType::ListItem && content.len == 0 {
            return self.outdent_blocks();
        }

        let b = self.boundary_from_point(&p);
        let (parent_of_new, idx) = self.lift_boundary(b, block);
        let new_data = match self.node(block).data.clone() {
            NodeData::Heading(_) if at_end => NodeData::Paragraph,
            NodeData::Quote => NodeData::Paragraph,
            NodeData::ListItem { .. } => {
                let kind = self.list_type_of_item(block);
                NodeData::ListItem { checked: (kind == Some(ListType::Check)).then_some(false) }
            }
            d => d,
        };
        let new_block = self.create_node(new_data);
        {
            let (indent, align) = (self.node(block).indent, self.node(block).align);
            let n = self.node_mut(new_block);
            n.indent = indent;
            n.align = align;
        }
        self.insert_after(block, new_block);
        let moving: Vec<_> = self.node(parent_of_new).children[idx..].to_vec();
        for k in moving {
            self.append_child(new_block, k);
        }
        let caret = self.prefer_text_point(new_block, 0);
        self.set_caret(caret);
        if self.node(new_block).children.is_empty() {
            self.selection.as_mut().unwrap().format = pending;
        }
        Ok(())
    }

    /// Split inline ancestors (links) of the boundary until it sits directly inside
    /// `block`; returns `(block, index)`.
    fn lift_boundary(&mut self, mut b: Boundary, block: NodeKey) -> (NodeKey, usize) {
        while b.parent != block {
            let parent = b.parent;
            let idx = self.boundary_index(&b);
            let clone = self.create_node(self.node(parent).data.clone());
            self.insert_after(parent, clone);
            let tail: Vec<_> = self.node(parent).children[idx..].to_vec();
            for k in tail {
                self.append_child(clone, k);
            }
            b = Boundary { parent: self.parent(parent).unwrap(), before: Some(clone) };
        }
        (block, self.boundary_index(&b))
    }

    // ---------------------------------------------------------------- format

    pub fn format_text(&mut self, flag: TextFormat) -> Result<()> {
        let sel = self.require_selection()?;
        if sel.is_collapsed() {
            let s = self.selection.as_mut().unwrap();
            s.format = s.format.toggled(flag);
            return Ok(());
        }
        let backward = self.is_backward();
        let (start, end) = self.ordered_points()?;
        let (a, b) = self.split_range(&start, &end);
        let texts: Vec<_> =
            self.leaves_between(&a, &b).into_iter().filter(|&k| self.node(k).is_text()).collect();
        if texts.is_empty() {
            return Ok(());
        }
        let all_have = texts.iter().all(|&k| self.node(k).text_format().contains(flag));
        for &k in &texts {
            let f = self.node(k).text_format();
            let nf = if all_have { f - flag } else { f.union(flag).toggled_exclusive(flag) };
            self.set_text_format(k, nf);
        }
        let (first, last) = (texts[0], *texts.last().unwrap());
        let (sp, ep) = (Point::text(first, 0), Point::text(last, self.node(last).text_len()));
        let s = self.selection.as_mut().unwrap();
        let fmt = self.nodes[&first].text_format();
        if backward {
            s.anchor = ep;
            s.focus = sp;
        } else {
            s.anchor = sp;
            s.focus = ep;
        }
        s.format = fmt;
        Ok(())
    }

    /// Common formatting of the selected text (or pending format when collapsed).
    pub fn selection_format(&self) -> TextFormat {
        let Some(sel) = &self.selection else { return TextFormat::empty() };
        if sel.is_collapsed() {
            return sel.format;
        }
        let Ok((s, e)) = self.ordered_points() else { return TextFormat::empty() };
        let mut me = self.clone();
        let (a, b) = me.split_range(&s, &e);
        let mut acc: Option<TextFormat> = None;
        for k in me.leaves_between(&a, &b) {
            if me.node(k).is_text() {
                let f = me.node(k).text_format();
                acc = Some(acc.map_or(f, |x| x & f));
            }
        }
        acc.unwrap_or_default()
    }

    pub fn selected_text(&self) -> String {
        let Ok((s, e)) = self.ordered_points() else { return String::new() };
        let blocks = self.line_blocks();
        let (Some(bs), Some(be)) =
            (self.line_block_of(self.inline_point(&s).key), self.line_block_of(self.inline_point(&e).key))
        else {
            return String::new();
        };
        let (Some(i), Some(j)) =
            (blocks.iter().position(|&b| b == bs), blocks.iter().position(|&b| b == be))
        else {
            return String::new();
        };
        let mut out = vec![];
        for (n, &b) in blocks[i..=j].iter().enumerate() {
            let c = self.block_content(b);
            let from = if n == 0 { self.block_offset(&c, &self.inline_point(&s)) } else { 0 };
            let to = if b == be { self.block_offset(&c, &self.inline_point(&e)) } else { c.len };
            out.push(c.text.chars().skip(from).take(to.saturating_sub(from)).collect::<String>());
        }
        out.join("\n")
    }
}

impl TextFormat {
    /// After setting `flag`, drop its mutually-exclusive partner.
    pub(crate) fn toggled_exclusive(self, flag: TextFormat) -> TextFormat {
        let mut out = self;
        if flag == TextFormat::SUBSCRIPT {
            out.remove(TextFormat::SUPERSCRIPT);
        } else if flag == TextFormat::SUPERSCRIPT {
            out.remove(TextFormat::SUBSCRIPT);
        }
        out
    }
}

/// Char offset reached by moving `granularity` from `off` in `text`.
pub(crate) fn step_offset(text: &str, off: usize, backward: bool, gran: Granularity) -> usize {
    let byte = byte_index(text, off);
    let to_chars = |b: usize| text[..b].chars().count();
    match gran {
        Granularity::Character => {
            let idx: Vec<usize> = text.grapheme_indices(true).map(|(i, _)| i).chain([text.len()]).collect();
            let b = if backward {
                idx.iter().rev().find(|&&i| i < byte).copied().unwrap_or(0)
            } else {
                idx.iter().find(|&&i| i > byte).copied().unwrap_or(text.len())
            };
            to_chars(b)
        }
        Granularity::Word => {
            let segs: Vec<(usize, &str)> = text.split_word_bound_indices().collect();
            if backward {
                let mut b = byte;
                for (i, s) in segs.iter().rev().filter(|(i, s)| i + s.len() <= byte) {
                    b = *i;
                    if !s.trim().is_empty() {
                        break;
                    }
                }
                to_chars(b)
            } else {
                let mut b = byte;
                for (i, s) in segs.iter().filter(|(i, _)| *i >= byte) {
                    b = i + s.len();
                    if !s.trim().is_empty() {
                        break;
                    }
                }
                to_chars(b)
            }
        }
        Granularity::Line => {
            if backward {
                text[..byte].rfind('\n').map_or(0, |i| to_chars(i + 1))
            } else {
                text[byte..].find('\n').map_or(text.chars().count(), |i| to_chars(byte + i))
            }
        }
    }
}
