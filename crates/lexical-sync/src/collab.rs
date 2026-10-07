//! Attaching a [`SyncDoc`] to an [`Editor`].
//!
//! * local edits: a commit hook mirrors every committed content change into the CRDT,
//!   whichever path produced it (commands, the toolbar, plugins);
//! * remote updates: [`Collab::receive`] imports them and swaps the editor's document,
//!   keeping the selection on the same characters;
//! * undo/redo: intercepted and routed to the CRDT's *local-only* undo, because a
//!   snapshot undo would also revert other peers' work.

use crate::doc::{SyncDoc, SyncOptions};
use crate::error::{Result, SyncError};
use crate::flat::{flat_offset, point_at_flat};
use lexical_core::{Command, Editor, EditorState, Selection, TextFormat, Tag};
use loro::cursor::Cursor;
use std::cell::RefCell;
use std::rc::Rc;

/// Handle to a document attached to an editor. Cheap to clone.
#[derive(Clone)]
pub struct Collab {
    pub(crate) doc: Rc<RefCell<SyncDoc>>,
    errors: Rc<RefCell<Vec<SyncError>>>,
}

/// A selection expressed as CRDT cursors so it survives remote edits.
struct Anchors {
    anchor: Option<Cursor>,
    focus: Option<Cursor>,
    format: TextFormat,
    style: String,
}

fn capture(doc: &SyncDoc, state: &EditorState) -> Option<Anchors> {
    let sel = state.selection.as_ref()?;
    Some(Anchors {
        anchor: doc.cursor_at(flat_offset(state, &sel.anchor)),
        focus: doc.cursor_at(flat_offset(state, &sel.focus)),
        format: sel.format,
        style: sel.style.clone(),
    })
}

/// Put the selection described by `anchors` onto `state`, a newer version of the document.
fn restore(doc: &SyncDoc, anchors: Option<Anchors>, mut state: EditorState) -> EditorState {
    if let Some(a) = anchors {
        let resolve = |c: &Option<Cursor>| c.as_ref().and_then(|c| doc.resolve_cursor(c));
        if let (Some(anchor), Some(focus)) = (resolve(&a.anchor), resolve(&a.focus)) {
            state.selection = Some(Selection {
                anchor: point_at_flat(&state, anchor),
                focus: point_at_flat(&state, focus),
                format: a.format,
                style: a.style,
            });
        }
    }
    state
}

impl Collab {
    /// Start a new collaborative document from the editor's current content (which becomes
    /// this peer's own edits) and wire the editor to it.
    pub fn attach(editor: &mut Editor, peer: u64) -> Result<Collab> {
        Self::attach_with(editor, peer, SyncOptions::default())
    }

    pub fn attach_with(editor: &mut Editor, peer: u64, options: SyncOptions) -> Result<Collab> {
        let mut doc = SyncDoc::with_options(peer, options)?;
        doc.apply_local(editor.state())?;
        Ok(Self::wire(editor, doc))
    }

    /// Join an existing document from a snapshot; the editor's content is replaced.
    pub fn join(editor: &mut Editor, peer: u64, snapshot: &[u8]) -> Result<Collab> {
        let doc = SyncDoc::join(peer, snapshot)?;
        let state = doc.state();
        let collab = Self::wire(editor, doc);
        editor.apply_remote(state);
        Ok(collab)
    }

    fn wire(editor: &mut Editor, doc: SyncDoc) -> Collab {
        let collab = Collab { doc: Rc::new(RefCell::new(doc)), errors: Rc::default() };

        let (doc, errors) = (collab.doc.clone(), collab.errors.clone());
        editor.register_commit_hook(move |info| {
            if info.content_changed
                && !info.tags.contains(&Tag::Remote)
                && let Err(e) = doc.borrow_mut().apply_local(info.state)
            {
                errors.borrow_mut().push(e);
            }
        });

        let (doc, errors) = (collab.doc.clone(), collab.errors.clone());
        editor.register_command(1000, move |ed, cmd| {
            let undo = match cmd {
                Command::Undo => true,
                Command::Redo => false,
                _ => return false,
            };
            if !ed.is_editable() {
                return false;
            }
            let next = {
                let mut d = doc.borrow_mut();
                let anchors = capture(&d, ed.state());
                match if undo { d.undo() } else { d.redo() } {
                    Ok(true) => Some(restore(&d, anchors, d.state())),
                    Ok(false) => None,
                    Err(e) => {
                        errors.borrow_mut().push(e);
                        None
                    }
                }
            };
            if let Some(state) = next {
                ed.apply_remote(state);
            }
            true
        });

        let doc = collab.doc.clone();
        editor.set_history_provider(Some(move || {
            let d = doc.borrow();
            (d.can_undo(), d.can_redo())
        }));
        collab
    }

    pub fn peer(&self) -> u64 {
        self.doc.borrow().peer()
    }

    /// Apply a remote update to `editor`.
    pub fn receive(&self, editor: &mut Editor, update: &[u8]) -> Result<()> {
        let next = {
            let mut doc = self.doc.borrow_mut();
            let anchors = capture(&doc, editor.state());
            if !doc.import(update)? {
                return Ok(());
            }
            restore(&doc, anchors, doc.state())
        };
        editor.apply_remote(next);
        Ok(())
    }

    /// Updates produced by local edits since the last call, to send to peers.
    pub fn drain_updates(&self) -> Vec<Vec<u8>> {
        self.doc.borrow().drain_updates()
    }

    pub fn snapshot(&self) -> Result<Vec<u8>> {
        self.doc.borrow().snapshot()
    }

    /// This replica's version vector (opaque bytes) for anti-entropy.
    pub fn version(&self) -> Vec<u8> {
        self.doc.borrow().version()
    }

    /// What a peer at `version` is missing.
    pub fn updates_since(&self, version: &[u8]) -> Result<Vec<u8>> {
        self.doc.borrow().updates_since(version)
    }

    /// Errors raised while mirroring local edits (the commit hook cannot return them).
    pub fn take_errors(&self) -> Vec<SyncError> {
        std::mem::take(&mut *self.errors.borrow_mut())
    }

    pub(crate) fn doc(&self) -> &Rc<RefCell<SyncDoc>> {
        &self.doc
    }
}

/// An editor bundled with its collaboration handle, for headless use and tests.
pub struct Replica {
    pub editor: Editor,
    pub collab: Collab,
}

impl Replica {
    pub fn new(peer: u64) -> Result<Replica> {
        Self::with_options(peer, SyncOptions::default())
    }

    pub fn with_options(peer: u64, options: SyncOptions) -> Result<Replica> {
        let mut editor = Editor::new();
        let collab = Collab::attach_with(&mut editor, peer, options)?;
        Ok(Replica { editor, collab })
    }

    pub fn join(peer: u64, snapshot: &[u8]) -> Result<Replica> {
        let mut editor = Editor::new();
        let collab = Collab::join(&mut editor, peer, snapshot)?;
        Ok(Replica { editor, collab })
    }

    pub fn dispatch(&mut self, cmd: Command) -> bool {
        self.editor.dispatch(cmd)
    }

    pub fn receive(&mut self, update: &[u8]) -> Result<()> {
        self.collab.receive(&mut self.editor, update)
    }

    pub fn drain_updates(&self) -> Vec<Vec<u8>> {
        self.collab.drain_updates()
    }

    pub fn text(&self) -> String {
        self.editor.state().to_plain_text()
    }

    pub fn json(&self) -> serde_json::Value {
        self.editor.state().to_json()
    }
}
