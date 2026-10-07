//! Verified model of `lexical_core::history::History` and the undo/redo plumbing in
//! `Editor` (`crates/lexical-core/src/{history,editor}.rs`).
//!
//! States are abstracted to `Copy` values; the real code stores `EditorState`
//! snapshots, which behaves identically as far as stack discipline goes.
//!
//! Proven:
//!  * `undo_len + redo_len <= limit` is invariant across every operation,
//!  * recording a content change always empties the redo stack,
//!  * `undo` restores exactly the state saved by `record`,
//!  * `undo` followed by `redo` (and vice versa) is the identity on the whole
//!    editor (current state and both stacks).
use vstd::prelude::*;

verus! {

pub struct Editor<T> {
    pub cur: T,
    pub undo: Vec<T>,
    pub redo: Vec<T>,
    pub limit: usize,
}

impl<T: Copy> Editor<T> {
    pub open spec fn wf(&self) -> bool {
        self.limit > 0 && self.undo.len() + self.redo.len() <= self.limit
    }

    pub open spec fn can_undo(&self) -> bool { self.undo.len() > 0 }

    pub open spec fn can_redo(&self) -> bool { self.redo.len() > 0 }

    /// A content change `cur -> next` was committed. `merge` is the already-decided
    /// coalescing flag (`!force_push && !undo.is_empty() && (force_merge || ...)`).
    pub fn commit(&mut self, next: T, merge: bool)
        requires
            old(self).wf(),
            // The merge decision in `History::record` requires a non-empty undo stack.
            merge ==> old(self).undo.len() > 0,
        ensures
            self.wf(),
            self.cur == next,
            self.limit == old(self).limit,
            self.redo@.len() == 0,
            merge ==> self.undo@ == old(self).undo@,
            !merge && old(self).undo.len() < old(self).limit ==>
                self.undo@ == old(self).undo@.push(old(self).cur),
            // When full, the oldest entry is dropped and the newest is the previous state.
            !merge && old(self).undo.len() >= old(self).limit ==> {
                &&& self.undo.len() == old(self).limit
                &&& self.undo@.last() == old(self).cur
            },
    {
        let prev = self.cur;
        self.cur = next;
        if !merge {
            self.undo.push(prev);
            if self.undo.len() > self.limit {
                self.undo.remove(0);
            }
        }
        self.redo = Vec::new();
    }

    pub fn undo(&mut self) -> (done: bool)
        requires old(self).wf(),
        ensures
            self.wf(),
            self.limit == old(self).limit,
            done == old(self).can_undo(),
            done ==> {
                &&& self.cur == old(self).undo@.last()
                &&& self.undo@ == old(self).undo@.drop_last()
                &&& self.redo@ == old(self).redo@.push(old(self).cur)
            },
            !done ==> self.cur == old(self).cur && self.undo@ == old(self).undo@
                && self.redo@ == old(self).redo@,
    {
        if self.undo.len() == 0 {
            return false;
        }
        let snap = self.undo.pop().unwrap();
        let current = self.cur;
        self.redo.push(current);
        self.cur = snap;
        true
    }

    pub fn redo(&mut self) -> (done: bool)
        requires old(self).wf(),
        ensures
            self.wf(),
            self.limit == old(self).limit,
            done == old(self).can_redo(),
            done ==> {
                &&& self.cur == old(self).redo@.last()
                &&& self.redo@ == old(self).redo@.drop_last()
                &&& self.undo@ == old(self).undo@.push(old(self).cur)
            },
            !done ==> self.cur == old(self).cur && self.undo@ == old(self).undo@
                && self.redo@ == old(self).redo@,
    {
        if self.redo.len() == 0 {
            return false;
        }
        let snap = self.redo.pop().unwrap();
        let current = self.cur;
        self.undo.push(current);
        self.cur = snap;
        true
    }
}

/// Undo then redo is the identity on the whole editor.
pub fn undo_redo_roundtrip<T: Copy>(e: &mut Editor<T>)
    requires old(e).wf(), old(e).can_undo(),
    ensures
        e.cur == old(e).cur,
        e.undo@ == old(e).undo@,
        e.redo@ == old(e).redo@,
{
    let a = e.undo();
    assert(a);
    let b = e.redo();
    assert(b);
    assert(e.undo@ =~= old(e).undo@);
    assert(e.redo@ =~= old(e).redo@);
}

/// Redo then undo is the identity as well.
pub fn redo_undo_roundtrip<T: Copy>(e: &mut Editor<T>)
    requires old(e).wf(), old(e).can_redo(),
    ensures
        e.cur == old(e).cur,
        e.undo@ == old(e).undo@,
        e.redo@ == old(e).redo@,
{
    let a = e.redo();
    assert(a);
    let b = e.undo();
    assert(b);
    assert(e.undo@ =~= old(e).undo@);
    assert(e.redo@ =~= old(e).redo@);
}

/// A new edit after an undo discards redo (no "branching" history).
pub fn edit_after_undo_clears_redo<T: Copy>(e: &mut Editor<T>, next: T)
    requires old(e).wf(), old(e).can_undo(),
    ensures e.redo@.len() == 0, e.cur == next,
{
    let _ = e.undo();
    e.commit(next, false);
}

} // verus!
