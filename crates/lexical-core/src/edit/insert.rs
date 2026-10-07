//! Typing: inserting text, pasted multi-line text, and line breaks.

use crate::error::Result;
use crate::node::*;
use crate::selection::{Point, PointKind};
use crate::state::EditorState;

impl EditorState {
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
}
