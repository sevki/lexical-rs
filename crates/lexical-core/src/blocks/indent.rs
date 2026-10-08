//! Indent / outdent of the selected blocks. List items nest (see `lists`); other
//! blocks carry an `indent` level. Both can be capped through [`Limits`](crate::Limits);
//! by default nothing is capped.

use crate::error::Result;
use crate::node::*;
use crate::state::EditorState;

impl EditorState {
    pub fn indent_blocks(&mut self) -> Result<()> {
        for b in self.selected_blocks() {
            if self.node(b).node_type() == NodeType::ListItem {
                self.indent_item(b);
            } else if self
                .limits
                .max_indent
                .is_none_or(|max| self.node(b).indent < max)
            {
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
}
