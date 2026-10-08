//! Block-level operations on the line blocks touched by the selection.

mod indent;
mod links;
mod lists;

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

impl EditorState {
    /// Line blocks touched by the selection, in document order.
    pub fn selected_blocks(&self) -> Vec<NodeKey> {
        let Ok((s, e)) = self.ordered_points() else {
            return vec![];
        };
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

    /// Block type of the first selected block, for toolbar state.
    pub fn current_block(&self) -> Option<NodeKey> {
        self.selected_blocks().first().copied()
    }

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
}
