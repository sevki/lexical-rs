//! Document plugins: components that take the whole document and return the whole
//! document (WIT interface `document-plugin`). They let code written for another editor
//! model, such as Lexical for JavaScript behind a shim, run against this editor.

use crate::backend::DocumentBackend;
use crate::error::{record, PluginError};
use lexical_core::{Command, Editor, EditorState, ListenerId, Plugin, Point, Selection, TextFormat};
use lexical_plugin::document as w;
use std::cell::RefCell;
use std::rc::Rc;

/// A document plugin component on some runtime, added to an editor with
/// [`Editor::add_plugin`].
///
/// For each command it understands, the plugin is handed the document and the selection and
/// answers with the document it wants; the editor then applies that as one ordinary update
/// (so history, listeners and collaboration all see it). Commands it has no name for, and
/// commands it neither handles nor changes the document for, go on to the next handler.
pub struct ComponentDocumentPlugin<B> {
    backend: Rc<RefCell<B>>,
    priority: i32,
    errors: Rc<RefCell<Vec<PluginError>>>,
    registered: Vec<ListenerId>,
}

impl<B: DocumentBackend + 'static> ComponentDocumentPlugin<B> {
    /// `priority`: higher runs first; the editor's own behaviour runs after every plugin.
    pub fn new(backend: B, priority: i32) -> Self {
        ComponentDocumentPlugin { backend: Rc::new(RefCell::new(backend)), priority, errors: Rc::default(), registered: vec![] }
    }

    /// Failures of calls into the plugin; a failing call leaves the editor untouched.
    pub fn errors(&self) -> Rc<RefCell<Vec<PluginError>>> {
        self.errors.clone()
    }
}

/// The plugin's name for a command, if it has one.
fn command_name(cmd: &Command) -> Option<(&'static str, String)> {
    Some(match cmd {
        Command::InsertText(t) if !t.contains('\n') => ("insert-text", t.clone()),
        Command::InsertParagraph => ("insert-paragraph", String::new()),
        Command::DeleteCharacter { backward: true } => ("delete-backward", String::new()),
        Command::DeleteCharacter { backward: false } => ("delete-forward", String::new()),
        Command::FormatText(f) => ("format-text", single_format(*f)?.to_string()),
        _ => return None,
    })
}

fn single_format(f: TextFormat) -> Option<&'static str> {
    const NAMES: [(TextFormat, &str); 8] = [
        (TextFormat::BOLD, "bold"),
        (TextFormat::ITALIC, "italic"),
        (TextFormat::STRIKETHROUGH, "strikethrough"),
        (TextFormat::UNDERLINE, "underline"),
        (TextFormat::CODE, "code"),
        (TextFormat::SUBSCRIPT, "subscript"),
        (TextFormat::SUPERSCRIPT, "superscript"),
        (TextFormat::HIGHLIGHT, "highlight"),
    ];
    NAMES.iter().find(|(flag, _)| *flag == f).map(|(_, name)| *name)
}

fn position_of(state: &EditorState, point: &Point) -> Option<w::Position> {
    let point = state.inline_point(point);
    let block = state.line_block_of(point.key)?;
    let index = state.line_blocks().iter().position(|&b| b == block)?;
    let offset = state.block_offset(&state.block_content(block), &point);
    Some(w::Position { block: index as u32, offset: offset as u32 })
}

fn point_of(state: &EditorState, pos: &w::Position) -> Option<Point> {
    let block = *state.line_blocks().get(pos.block as usize)?;
    Some(state.block_point(block, &state.block_content(block), pos.offset as usize))
}

fn selection_of(state: &EditorState) -> Option<w::DocumentSelection> {
    let sel = state.selection.as_ref()?;
    Some(w::DocumentSelection { anchor: position_of(state, &sel.anchor)?, focus: position_of(state, &sel.focus)? })
}

impl<B: DocumentBackend + 'static> Plugin for ComponentDocumentPlugin<B> {
    fn set_up(&mut self, editor: &mut Editor) {
        let (backend, errors) = (self.backend.clone(), self.errors.clone());
        let id = editor.register_command(self.priority, move |ed, cmd| {
            let Some((name, payload)) = command_name(cmd) else { return false };
            if !ed.is_editable() {
                return false;
            }
            let before = ed.state().to_json();
            let input = ed.state().to_json_string();
            let selection = selection_of(ed.state());
            let outcome = backend.borrow_mut().run(&input, selection, name, &payload);
            let outcome = match outcome {
                Ok(o) => o,
                Err(e) => {
                    record(&errors, e);
                    return false;
                }
            };
            let mut next = match EditorState::from_json_str(&outcome.state) {
                Ok(s) => s,
                Err(e) => {
                    record(&errors, PluginError::Call(format!("the plugin returned an invalid document: {e}")));
                    return false;
                }
            };
            if !outcome.handled && next.to_json() == before {
                return false;
            }
            let old = ed.state().selection.clone();
            let anchor = outcome.selection.as_ref().and_then(|s| point_of(&next, &s.anchor));
            let focus = outcome.selection.as_ref().and_then(|s| point_of(&next, &s.focus));
            next.selection = match (anchor, focus) {
                (Some(anchor), Some(focus)) => {
                    let mut sel = Selection::new(anchor, focus);
                    if let Some(old) = old {
                        sel.format = old.format;
                        sel.style = old.style;
                    }
                    Some(sel)
                }
                _ => next.line_blocks().first().map(|&b| Selection::collapsed(Point::element(b, 0))),
            };
            let _ = ed.update(|s| {
                s.replace_document(next);
                Ok(())
            });
            true
        });
        self.registered.push(id);
    }

    fn tear_down(&mut self, editor: &mut Editor) {
        for id in self.registered.drain(..) {
            editor.unregister(id);
        }
    }
}
