//! Deleting: selected ranges (with block merging) and character / word / line steps.

use super::Boundary;
use crate::error::Result;
use crate::node::*;
use crate::selection::{Point, Selection};
use crate::state::EditorState;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Granularity {
    Character,
    Word,
    Line,
}

impl EditorState {
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
        if let (Some(ba), Some(bb)) = (blk_a, blk_b)
            && ba != bb
        {
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
                    return self.set_block_type(crate::blocks::BlockType::Paragraph);
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
}

/// Char offset reached by moving `granularity` from `off` in `text`.
pub(crate) fn step_offset(text: &str, off: usize, backward: bool, gran: Granularity) -> usize {
    let byte = byte_index(text, off);
    let to_chars = |b: usize| text[..b].chars().count();
    match gran {
        Granularity::Character => {
            let idx: Vec<usize> =
                text.grapheme_indices(true).map(|(i, _)| i).chain([text.len()]).collect();
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
