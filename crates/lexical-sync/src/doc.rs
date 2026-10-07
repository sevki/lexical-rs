//! [`SyncDoc`]: a Loro document holding the flattened Lexical document as one rich text.
//!
//! Invariant: `last` always equals the CRDT's current content, so a local edit can be
//! applied as the minimal difference between `last` and the edited document.

use crate::error::{crdt, Result, SyncError};
use crate::flat::{flat_offset, flatten, Flat};
use crate::marks::{self, MarkValue, Marks};
use crate::unflatten::unflatten;
use lexical_core::EditorState;
use loro::cursor::{Cursor, Side};
use loro::{
    ExpandType, ExportMode, LoroDoc, LoroText, LoroValue, StyleConfig, Subscription, TextDelta, UndoManager,
    VersionVector,
};
use std::sync::{Arc, Mutex};

const TEXT_ID: &str = "lexical";

/// Peer id used only to author the initial empty line, so every replica that starts
/// from scratch creates the *identical* operation and merges to a single first line
/// instead of one each. Real peers must not use it.
pub const BOOTSTRAP_PEER: u64 = 0x1E71_CA1B_0075_7AA9;

#[derive(Clone, Copy, Debug)]
pub struct SyncOptions {
    /// Local edits closer together than this share one undo step (milliseconds).
    pub undo_merge_ms: i64,
}

impl Default for SyncOptions {
    fn default() -> Self {
        SyncOptions { undo_merge_ms: 1000 }
    }
}

pub struct SyncDoc {
    doc: LoroDoc,
    text: LoroText,
    last: Flat,
    undo: UndoManager,
    outbox: Arc<Mutex<Vec<Vec<u8>>>>,
    _local_updates: Subscription,
}

impl SyncDoc {
    /// A new replica of the shared initial document (one empty paragraph).
    pub fn new(peer: u64) -> Result<SyncDoc> {
        Self::with_options(peer, SyncOptions::default())
    }

    pub fn with_options(peer: u64, options: SyncOptions) -> Result<SyncDoc> {
        if peer == BOOTSTRAP_PEER {
            return Err(SyncError::ReservedPeer);
        }
        let doc = LoroDoc::new();
        configure(&doc);
        // Identical bytes on every replica: no timestamp, fixed peer, fixed content.
        doc.set_peer_id(BOOTSTRAP_PEER).map_err(crdt)?;
        doc.get_text(TEXT_ID).insert(0, "\n").map_err(crdt)?;
        doc.commit();
        doc.set_peer_id(peer).map_err(crdt)?;
        Self::finish(doc, options)
    }

    /// Join an existing document from a snapshot exported by another replica.
    pub fn join(peer: u64, snapshot: &[u8]) -> Result<SyncDoc> {
        if peer == BOOTSTRAP_PEER {
            return Err(SyncError::ReservedPeer);
        }
        let doc = LoroDoc::from_snapshot(snapshot).map_err(crdt)?;
        configure(&doc);
        doc.set_peer_id(peer).map_err(crdt)?;
        Self::finish(doc, SyncOptions::default())
    }

    fn finish(doc: LoroDoc, options: SyncOptions) -> Result<SyncDoc> {
        let text = doc.get_text(TEXT_ID);
        let outbox = Arc::new(Mutex::new(Vec::new()));
        let sink = outbox.clone();
        let sub = doc.subscribe_local_update(Box::new(move |bytes: &Vec<u8>| {
            sink.lock().expect("outbox poisoned").push(bytes.clone());
            true
        }));
        let mut undo = UndoManager::new(&doc);
        undo.set_merge_interval(options.undo_merge_ms);
        let last = read_flat(&text);
        Ok(SyncDoc { doc, text, last, undo, outbox, _local_updates: sub })
    }

    pub fn peer(&self) -> u64 {
        self.doc.peer_id()
    }

    /// The current document, rebuilt from the CRDT.
    pub fn state(&self) -> EditorState {
        unflatten(&self.last)
    }

    pub fn flat(&self) -> &Flat {
        &self.last
    }

    /// Bring the CRDT in line with a locally edited document.
    pub fn apply_local(&mut self, state: &EditorState) -> Result<()> {
        let next = flatten(state);
        if next == self.last {
            return Ok(());
        }
        let caret = state.selection.as_ref().map(|s| flat_offset(state, &s.focus));
        let edit = minimal_edit(&self.last.chars, &next.chars);
        let (p, old_end, new_end) = align_edit(&self.last.chars, &next.chars, edit, caret);
        if old_end > p {
            self.text.delete(p, old_end - p).map_err(crdt)?;
        }
        if new_end > p {
            let inserted: String = next.chars[p..new_end].iter().collect();
            self.text.insert(p, &inserted).map_err(crdt)?;
        }
        // Text inserted inside a marked range inherits its marks, so read back what the
        // CRDT really holds and correct only the differences.
        let current = read_raw(&self.text);
        debug_assert_eq!(current.len(), next.chars.len());
        self.reconcile_marks(&current, &next)?;
        self.doc.commit();
        debug_assert_eq!(read_flat(&self.text), next, "CRDT content must equal the edited document");
        self.last = next;
        Ok(())
    }

    /// Compare the *raw* marks the CRDT holds with the desired ones. Decoding hides marks
    /// that do not apply (a `block` mark under a `list` mark, say) but a concurrent change
    /// could make them visible later, so they are cleared too.
    fn reconcile_marks(&self, cur: &[Marks], desired: &Flat) -> Result<()> {
        let want: Vec<Marks> = desired.attrs.iter().map(marks::encode).collect();
        let n = cur.len();
        for key in marks::KEYS {
            let mut i = 0;
            while i < n {
                let (c, d) = (cur[i].get(key), want[i].get(key));
                if c == d {
                    i += 1;
                    continue;
                }
                let mut j = i + 1;
                while j < n && want[j].get(key) == d && cur[j].get(key) != d {
                    j += 1;
                }
                match d {
                    Some(v) => self.text.mark(i..j, key, to_loro(v)).map_err(crdt)?,
                    None => self.text.unmark(i..j, key).map_err(crdt)?,
                }
                i = j;
            }
        }
        Ok(())
    }

    /// Apply a remote update. Returns whether the document content changed.
    pub fn import(&mut self, bytes: &[u8]) -> Result<bool> {
        self.doc.import(bytes).map_err(crdt)?;
        Ok(self.refresh())
    }

    fn refresh(&mut self) -> bool {
        let now = read_flat(&self.text);
        let changed = now != self.last;
        self.last = now;
        changed
    }

    /// Updates produced by local edits since the last call, ready to send to peers.
    pub fn drain_updates(&self) -> Vec<Vec<u8>> {
        std::mem::take(&mut *self.outbox.lock().expect("outbox poisoned"))
    }

    pub fn snapshot(&self) -> Result<Vec<u8>> {
        self.doc.export(ExportMode::Snapshot).map_err(crdt)
    }

    /// This replica's version, to send to a peer so it can reply with what is missing.
    pub fn version(&self) -> Vec<u8> {
        self.doc.oplog_vv().encode()
    }

    /// Everything the replica with `version` does not have yet.
    pub fn updates_since(&self, version: &[u8]) -> Result<Vec<u8>> {
        let vv = VersionVector::decode(version).map_err(|e| SyncError::BadMessage(e.to_string()))?;
        self.doc.export(ExportMode::updates(&vv)).map_err(crdt)
    }

    pub fn can_undo(&self) -> bool {
        self.undo.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.undo.can_redo()
    }

    /// Undo this replica's most recent edit; other peers' edits are untouched.
    pub fn undo(&mut self) -> Result<bool> {
        self.doc.commit();
        let did = self.undo.undo().map_err(crdt)?;
        self.refresh();
        Ok(did)
    }

    pub fn redo(&mut self) -> Result<bool> {
        self.doc.commit();
        let did = self.undo.redo().map_err(crdt)?;
        self.refresh();
        Ok(did)
    }

    /// A position that follows its character through concurrent edits.
    pub fn cursor_at(&self, flat_offset: usize) -> Option<Cursor> {
        self.text.get_cursor(flat_offset, Side::Middle)
    }

    /// Where a cursor points now, as a flat offset.
    pub fn resolve_cursor(&self, cursor: &Cursor) -> Option<usize> {
        self.doc.get_cursor_pos(cursor).ok().map(|r| r.current.pos)
    }
}

fn configure(doc: &LoroDoc) {
    // Wall-clock change timestamps are never used (undo merging has its own clock) and
    // mixing them with unstamped changes trips a debug assertion in Loro's change merging
    // on CI. Leaving them off everywhere also keeps the bootstrap bytes identical.
    doc.set_record_timestamp(false);
    // Marks never grow at their edges: formatting of newly typed text is stated explicitly
    // by the editor, so the result does not depend on which side a peer typed from.
    doc.config_default_text_style(Some(StyleConfig { expand: ExpandType::None }));
}

/// The marks of every character exactly as stored, restricted to the keys we know.
fn read_raw(text: &LoroText) -> Vec<Marks> {
    let mut out = Vec::new();
    for delta in text.to_delta() {
        let TextDelta::Insert { insert, attributes } = delta else { continue };
        let mut m = Marks::new();
        if let Some(attrs) = &attributes {
            for key in marks::KEYS {
                if let Some(v) = attrs.get(*key).and_then(from_loro) {
                    m.insert(key, v);
                }
            }
        }
        out.extend(insert.chars().map(|_| m.clone()));
    }
    out
}

fn read_flat(text: &LoroText) -> Flat {
    let mut flat = Flat::default();
    for delta in text.to_delta() {
        let TextDelta::Insert { insert, attributes } = delta else { continue };
        let get = |key: &str| attributes.as_ref().and_then(|a| a.get(key)).and_then(from_loro);
        for ch in insert.chars() {
            flat.chars.push(ch);
            flat.attrs.push(marks::decode(ch, &get));
        }
    }
    flat
}

fn to_loro(v: &MarkValue) -> LoroValue {
    match v {
        MarkValue::Bool(b) => LoroValue::from(*b),
        MarkValue::Int(i) => LoroValue::from(*i),
        MarkValue::Str(s) => LoroValue::from(s.as_str()),
    }
}

fn from_loro(v: &LoroValue) -> Option<MarkValue> {
    match v {
        LoroValue::Bool(b) => Some(MarkValue::Bool(*b)),
        LoroValue::I64(i) => Some(MarkValue::Int(*i)),
        LoroValue::String(s) => Some(MarkValue::Str(s.to_string())),
        _ => None,
    }
}

/// A text diff cannot tell deleting `" brave"` from `"brave "` (or typing the second `l` of
/// "hello" from the first): the results are identical but the CRDT operations are not, and
/// a concurrent peer's edit lands differently. Among the equivalent placements of a pure
/// insertion or deletion, pick the one that ends at the caret, which is where the user
/// actually made the edit.
fn align_edit(
    old: &[char],
    new: &[char],
    edit: (usize, usize, usize),
    caret: Option<usize>,
) -> (usize, usize, usize) {
    let (Some(caret), (p, old_end, new_end)) = (caret, edit) else { return edit };
    if new_end == p && old_end > p {
        let len = old_end - p;
        let mut lo = p;
        while lo > 0 && old[lo - 1] == old[lo - 1 + len] {
            lo -= 1;
        }
        let mut hi = p;
        while hi + len < old.len() && old[hi] == old[hi + len] {
            hi += 1;
        }
        let start = caret.clamp(lo, hi);
        (start, start + len, start)
    } else if old_end == p && new_end > p {
        let len = new_end - p;
        let mut lo = p;
        while lo > 0 && new[lo - 1] == new[lo - 1 + len] {
            lo -= 1;
        }
        let mut hi = p;
        while hi + len < new.len() && new[hi] == new[hi + len] {
            hi += 1;
        }
        let start = caret.saturating_sub(len).clamp(lo, hi);
        (start, start, start + len)
    } else {
        edit
    }
}

/// `(start, old_end, new_end)`: replace `old[start..old_end]` with `new[start..new_end]`.
fn minimal_edit(old: &[char], new: &[char]) -> (usize, usize, usize) {
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let max_suffix = old.len().min(new.len()) - prefix;
    let suffix = old.iter().rev().zip(new.iter().rev()).take(max_suffix).take_while(|(a, b)| a == b).count();
    (prefix, old.len() - suffix, new.len() - suffix)
}
