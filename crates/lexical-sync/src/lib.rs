//! Collaborative editing for [`lexical_core`] on top of the [Loro](https://loro.dev) CRDT.
//!
//! The document is flattened to a single rich text ([`flat`]) so typing, Enter and
//! Backspace are ordinary text inserts and deletes that a CRDT merges natively; block and
//! list structure travel as attributes on line terminators and are rebuilt by
//! [`unflatten`] (which is total: any merge result is a valid document).
//!
//! * [`SyncDoc`] – the Loro document and its mapping to/from an `EditorState`
//! * [`Collab`] – attaches a `SyncDoc` to an `Editor`: local edits flow out, remote
//!   updates flow in, undo is local-only, the selection follows concurrent edits
//! * [`Replica`] – an `Editor` + `Collab` bundle for headless use and tests
//! * [`testing`] – an in-memory cluster with duplicated / reordered / delayed delivery

pub mod collab;
pub mod doc;
pub mod error;
pub mod flat;
pub mod marks;
pub mod presence;
pub mod testing;
mod unflatten;

pub use collab::{Collab, Replica};
pub use doc::{SyncDoc, SyncOptions, BOOTSTRAP_PEER};
pub use error::{Result, SyncError};
pub use flat::{flatten, Flat};
pub use presence::RemoteCaret;
pub use unflatten::unflatten;
