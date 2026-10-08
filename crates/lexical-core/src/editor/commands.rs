//! The command vocabulary and its dispatch: registered handlers first (highest
//! priority wins), then the built-in behaviour.

use super::{Editor, Tag};
use crate::blocks::BlockType;
use crate::error::{Error, Result};
use crate::format::{Align, TextFormat};
use crate::history::ChangeKind;
use crate::node::ListType;
use crate::state::EditorState;

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    InsertText(String),
    Paste(String),
    InsertParagraph,
    InsertLineBreak,
    DeleteCharacter { backward: bool },
    DeleteWord { backward: bool },
    DeleteLine { backward: bool },
    FormatText(TextFormat),
    FormatAlign(Align),
    SetBlockType(BlockType),
    ToggleList(ListType),
    ToggleCheck,
    ToggleLink(Option<String>),
    Indent,
    Outdent,
    SelectAll,
    Undo,
    Redo,
    Custom(String),
}

impl Editor {
    /// Dispatch a command: registered handlers first, then the built-in behaviour.
    /// Returns whether anything handled it.
    pub fn dispatch(&mut self, cmd: Command) -> bool {
        // Handlers stay registered while they run, so a command dispatched from inside one
        // reaches the others. A handler that is already running is skipped for the nested
        // command, which also rules out a handler recursing into itself.
        let handlers: Vec<_> = self.command_handlers.iter().map(|(_, _, h)| h.clone()).collect();
        let mut handled = false;
        for h in handlers {
            let Ok(mut f) = h.try_borrow_mut() else { continue };
            if f(self, &cmd) {
                handled = true;
                break;
            }
        }
        handled || self.handle_default(&cmd)
    }

    fn handle_default(&mut self, cmd: &Command) -> bool {
        use Command::*;
        // A read-only editor allows selection changes only; history would replace the
        // document, so it is gated like every other edit.
        if !self.editable && !matches!(cmd, SelectAll) {
            return false;
        }
        match cmd {
            Undo => return self.undo(),
            Redo => return self.redo(),
            _ => {}
        }
        let kind = match cmd {
            InsertText(t) if !t.contains('\n') => ChangeKind::Typing,
            DeleteCharacter { backward: true } => ChangeKind::DeleteBackward,
            DeleteCharacter { backward: false } => ChangeKind::DeleteForward,
            _ => ChangeKind::Other,
        };
        let tags = [Tag::Kind(kind)];
        let r = self.update_tagged(&tags, |s| match cmd {
            InsertText(t) | Paste(t) => {
                if matches!(cmd, Paste(_)) {
                    s.insert_raw_text(t)
                } else {
                    s.insert_text(t)
                }
            }
            InsertParagraph => s.insert_paragraph(),
            InsertLineBreak => s.insert_line_break(),
            DeleteCharacter { backward } => s.delete_character(*backward),
            DeleteWord { backward } => s.delete_word(*backward),
            DeleteLine { backward } => s.delete_line(*backward),
            FormatText(f) => s.format_text(*f),
            FormatAlign(a) => s.set_align(*a),
            SetBlockType(b) => s.set_block_type(*b),
            ToggleList(t) => s.toggle_list(*t),
            ToggleCheck => {
                for b in s.selected_blocks() {
                    s.toggle_check(b);
                }
                Ok(())
            }
            ToggleLink(u) => s.toggle_link(u.as_deref()),
            Indent => s.indent_blocks(),
            Outdent => s.outdent_blocks(),
            SelectAll => s.select_all(),
            Undo | Redo | Custom(_) => Err(Error::Invalid("unhandled".into())),
        });
        r.is_ok() && !matches!(cmd, Custom(_))
    }
}

impl EditorState {
    pub fn select_all(&mut self) -> Result<()> {
        let blocks = self.line_blocks();
        let (Some(&f), Some(&l)) = (blocks.first(), blocks.last()) else { return Ok(()) };
        let end = self.node(l).children.len();
        self.set_selection_points(crate::Point::element(f, 0), crate::Point::element(l, end));
        Ok(())
    }
}
