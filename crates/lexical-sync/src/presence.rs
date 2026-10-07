//! Presence: where each collaborator's caret is.
//!
//! The Loro Rust crate has no ephemeral-state channel, so presence is a small message the
//! host sends over its own transport. Carets are CRDT cursors, so a received caret lands on
//! the right characters even if edits arrived in between.

use crate::collab::Collab;
use crate::error::{Result, SyncError};
use crate::flat::{flat_offset, point_at_flat};
use lexical_core::{Editor, Point};
use loro::cursor::Cursor;
use serde_json::{json, Value};

/// A collaborator's caret, resolved against the current document.
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteCaret {
    pub peer: u64,
    pub name: String,
    pub color: String,
    pub anchor: Point,
    pub focus: Point,
}

impl Collab {
    /// Describe this peer's selection for others. `None` when there is no selection.
    pub fn local_presence(&self, editor: &Editor, name: &str, color: &str) -> Option<Vec<u8>> {
        let state = editor.state();
        let sel = state.selection.as_ref()?;
        let doc = self.doc().borrow();
        let anchor = doc.cursor_at(flat_offset(state, &sel.anchor))?;
        let focus = doc.cursor_at(flat_offset(state, &sel.focus))?;
        let msg = json!({
            "peer": doc.peer(),
            "name": name,
            "color": color,
            "anchor": anchor.encode(),
            "focus": focus.encode(),
        });
        serde_json::to_vec(&msg).ok()
    }

    /// Place a received presence message onto the current document.
    pub fn resolve_presence(&self, editor: &Editor, message: &[u8]) -> Result<RemoteCaret> {
        let bad = |what: &str| SyncError::BadMessage(what.to_string());
        let v: Value = serde_json::from_slice(message).map_err(|e| SyncError::BadMessage(e.to_string()))?;
        let cursor = |key: &str| -> Result<Cursor> {
            let bytes: Vec<u8> = v[key]
                .as_array()
                .ok_or_else(|| bad("missing cursor"))?
                .iter()
                .map(|b| b.as_u64().and_then(|b| u8::try_from(b).ok()).ok_or_else(|| bad("bad cursor byte")))
                .collect::<Result<_>>()?;
            Cursor::decode(&bytes).map_err(|e| SyncError::BadMessage(e.to_string()))
        };
        let (anchor, focus) = (cursor("anchor")?, cursor("focus")?);
        let doc = self.doc().borrow();
        let at = |c: &Cursor| doc.resolve_cursor(c).ok_or_else(|| bad("cursor not found in this document"));
        let state = editor.state();
        Ok(RemoteCaret {
            peer: v["peer"].as_u64().ok_or_else(|| bad("missing peer"))?,
            name: v["name"].as_str().unwrap_or_default().to_string(),
            color: v["color"].as_str().unwrap_or_default().to_string(),
            anchor: point_at_flat(state, at(&anchor)?),
            focus: point_at_flat(state, at(&focus)?),
        })
    }
}
