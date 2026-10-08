//! Enter: splitting blocks, with the per-kind rules for code blocks and list items.

use super::Boundary;
use crate::error::Result;
use crate::node::*;
use crate::selection::Point;
use crate::state::EditorState;

impl EditorState {
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
}
