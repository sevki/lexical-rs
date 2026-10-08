//! Yjs interop for [`lexical_core`]: read and write the document layout that Lexical for
//! JavaScript's `@lexical/yjs` (the collaboration binding behind `CollaborationPlugin`) keeps
//! in a [`yrs`] document, so a Rust editor and a Lexical web editor can share one document.
//!
//! The layout (binding v1, a top-level `XmlText` named `root`):
//!
//! * an element is an embedded `XmlText` whose attributes are the node's properties
//!   (`__type`, `__format`, `__indent`, `__tag`, ...) and whose content is its children;
//! * a text node is an embedded `Map` of properties (`__type: "text"`, `__format`, `__style`,
//!   `__mode`, `__detail`) followed by the node's characters in the same `XmlText`;
//! * a line break or any other leaf is an embedded `Map` with `__type` and its properties.
//!
//! [`YjsDoc`] converts that tree to and from Lexical JSON, so everything `lexical-core`
//! knows about JSON (including [`NodeData::Unknown`](lexical_core::NodeData::Unknown))
//! applies. Local changes are written as a minimal diff ([`YjsDoc::set_state`]) and the
//! resulting update can be sent to any Yjs peer.

//!
//! # Limits
//!
//! * Only binding v1 (`createBinding`, the one `CollaborationPlugin` uses) is supported, not
//!   the experimental `createBindingV2`.
//! * [`YjsDoc::set_state`] diffs by position: typing and formatting become small edits,
//!   but splitting or merging blocks rewrites the affected block, so a concurrent edit
//!   inside that block can be lost. Finer-grained block edits are future work.
//! * `textFormat` / `textStyle` and list-item indents are derived by Lexical for
//!   JavaScript and are not compared.
//! * Decorator nodes (images, ...) are kept as opaque leaves; the node types Lexical for
//!   JavaScript must have registered to render them are its concern.

mod diff;
mod tree;

use lexical_core::EditorState;
use thiserror::Error;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{Doc, Options, ReadTxn, StateVector, Text, TextRef, Transact, Update};
use yrs::sync::{Message, SyncMessage};
use yrs::OffsetKind;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid Yjs update: {0}")]
    Update(String),
    #[error("document does not hold a Lexical tree: {0}")]
    Layout(String),
    #[error(transparent)]
    Lexical(#[from] lexical_core::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Name of the top-level shared type `@lexical/yjs` binds to.
pub const ROOT_NAME: &str = "root";

/// A Yjs document holding one Lexical editor state.
pub struct YjsDoc {
    doc: Doc,
    root: TextRef,
}

impl Default for YjsDoc {
    fn default() -> Self {
        Self::new()
    }
}

impl YjsDoc {
    pub fn new() -> YjsDoc {
        // JavaScript peers count string offsets in UTF-16 units.
        let doc = Doc::with_options(Options { offset_kind: OffsetKind::Utf16, ..Options::default() });
        let root = doc.get_or_insert_text(ROOT_NAME);
        YjsDoc { doc, root }
    }

    /// Apply an update (Yjs v1 encoding) from a peer.
    pub fn apply_update(&self, update: &[u8]) -> Result<()> {
        let update = Update::decode_v1(update).map_err(|e| Error::Update(e.to_string()))?;
        self.doc.transact_mut().apply_update(update).map_err(|e| Error::Update(e.to_string()))
    }

    /// The whole document as one update.
    pub fn encode_state(&self) -> Vec<u8> {
        self.doc.transact().encode_state_as_update_v1(&StateVector::default())
    }

    /// What a peer with state vector `sv` (v1 encoding) is missing.
    pub fn encode_diff(&self, sv: &[u8]) -> Result<Vec<u8>> {
        let sv = StateVector::decode_v1(sv).map_err(|e| Error::Update(e.to_string()))?;
        Ok(self.doc.transact().encode_state_as_update_v1(&sv))
    }

    pub fn state_vector(&self) -> Vec<u8> {
        self.doc.transact().state_vector().encode_v1()
    }

    /// Read the document into an editor state.
    pub fn state(&self) -> Result<EditorState> {
        let txn = self.doc.transact();
        let json = tree::read_json(&txn, &self.root)?;
        Ok(EditorState::from_json(&json)?)
    }

    /// Make the document equal `state`, changing as little as possible, and return the
    /// update to send to peers (empty when nothing changed).
    pub fn set_state(&self, state: &EditorState) -> Result<Vec<u8>> {
        let before = self.doc.transact().state_vector();
        {
            let mut txn = self.doc.transact_mut();
            diff::sync(&mut txn, &self.root, &state.to_json())?;
        }
        let update = self.doc.transact().encode_state_as_update_v1(&before);
        Ok(update)
    }

    /// The y-protocols `sync step 1` message announcing what this document has; send it when
    /// a connection opens (y-websocket framing: the message type is part of the bytes).
    pub fn sync_step1(&self) -> Vec<u8> {
        Message::Sync(SyncMessage::SyncStep1(self.doc.transact().state_vector())).encode_v1()
    }

    /// An `update` message carrying `update`, for broadcasting a local change.
    pub fn update_message(update: Vec<u8>) -> Vec<u8> {
        Message::Sync(SyncMessage::Update(update)).encode_v1()
    }

    /// Handle one y-protocols sync message from a peer; returns the reply to send back, if
    /// any (a `sync step 2` for a `sync step 1`). Other message kinds (awareness, auth) are
    /// ignored.
    pub fn handle_message(&self, message: &[u8]) -> Result<Option<Vec<u8>>> {
        let msg = Message::decode_v1(message).map_err(|e| Error::Update(e.to_string()))?;
        let Message::Sync(sync) = msg else { return Ok(None) };
        match sync {
            SyncMessage::SyncStep1(sv) => {
                let diff = self.doc.transact().encode_state_as_update_v1(&sv);
                Ok(Some(Message::Sync(SyncMessage::SyncStep2(diff)).encode_v1()))
            }
            SyncMessage::SyncStep2(u) | SyncMessage::Update(u) => {
                self.apply_update(&u)?;
                Ok(None)
            }
        }
    }

    /// Length of the root in Yjs units; zero for a document nobody wrote to.
    pub fn is_empty(&self) -> bool {
        self.root.len(&self.doc.transact()) == 0
    }
}
