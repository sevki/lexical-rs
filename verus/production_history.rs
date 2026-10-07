//! Refinement of the production history (`crates/lexical-core/src/{history,editor}.rs`)
//! to the proven replay kernel (`kernels/replay/`).
//!
//! Production keeps the *present* in `Editor::state` and two stacks in `History`:
//! `undo` (oldest first, newest last) and `redo` (a stack: newest *undone* state last).
//! The kernel's `future` lists redo states nearest-first, so the abstraction function is
//!
//! ```text
//! abs(p) = History { past: p.undo, present: p.cur, future: reverse(p.redo) }
//! ```
//!
//! Proven: each production operation maps to exactly the corresponding kernel
//! operation under `abs`; hence every theorem of the kernel (RK1-RK4) holds for the
//! production history, and the size bound `undo.len + redo.len <= limit` is invariant.
use crate::kernels::replay::{redo, undo, History};
use vstd::prelude::*;

verus! {

pub struct Prod<T> {
    pub cur: T,
    pub undo: Vec<T>,
    pub redo: Vec<T>,
    pub limit: usize,
}

pub open spec fn abs<T>(p: Prod<T>) -> History<T> {
    History { past: p.undo@, present: p.cur, future: p.redo@.reverse() }
}

pub open spec fn wf<T>(p: Prod<T>) -> bool {
    p.limit > 0 && p.undo.len() + p.redo.len() <= p.limit
}

impl<T: Copy> Prod<T> {
    pub fn new(cur: T, limit: usize) -> (r: Self)
        requires limit > 0,
        ensures wf(r), r.cur == cur, r.undo@.len() == 0, r.redo@.len() == 0,
    {
        Prod { cur, undo: Vec::new(), redo: Vec::new(), limit }
    }

    /// `Editor::commit` for a content change `cur -> next`; `merge` is the coalescing
    /// decision already taken by `History::record`.
    pub fn commit(self, next: T, merge: bool) -> (r: Self)
        requires
            wf(self),
            merge ==> self.undo.len() > 0,
        ensures
            wf(r),
            r.limit == self.limit,
            r.cur == next,
            r.redo@.len() == 0,
            // merge == kernel `do_merge`: replace present, keep the undo stack, drop redo
            merge ==> abs(r) == (History { past: self.undo@, present: next, future: Seq::<T>::empty() }),
            // otherwise == kernel `do_action` (bounded): push the previous present
            !merge && self.undo.len() < self.limit ==>
                abs(r) == (History { past: self.undo@.push(self.cur), present: next, future: Seq::<T>::empty() }),
            !merge && self.undo.len() >= self.limit ==> {
                &&& r.undo.len() == self.limit
                &&& r.undo@ == self.undo@.push(self.cur).skip(1)
            },
    {
        let Prod { cur, mut undo, redo: _, limit } = self;
        if !merge {
            undo.push(cur);
            if undo.len() > limit {
                undo.remove(0);
            }
        }
        let r = Prod { cur: next, undo, redo: Vec::new(), limit };
        proof {
            assert(r.redo@.reverse() =~= Seq::<T>::empty());
        }
        r
    }

    /// `Editor::undo`
    pub fn undo(self) -> (r: Self)
        requires wf(self),
        ensures
            wf(r),
            r.limit == self.limit,
            abs(r) == undo(abs(self)),
    {
        let Prod { cur, mut undo, mut redo, limit } = self;
        if undo.len() == 0 {
            return Prod { cur, undo, redo, limit };
        }
        let snap = undo.pop().unwrap();
        redo.push(cur);
        let r = Prod { cur: snap, undo, redo, limit };
        proof {
            let a = abs(self);
            // future' = [cur] + reverse(redo)
            assert(r.redo@.reverse() =~= seq![self.cur] + self.redo@.reverse());
            assert(r.undo@ =~= a.past.drop_last());
        }
        r
    }

    /// `Editor::redo`
    pub fn redo(self) -> (r: Self)
        requires wf(self),
        ensures
            wf(r),
            r.limit == self.limit,
            abs(r) == redo(abs(self)),
    {
        let Prod { cur, mut undo, mut redo, limit } = self;
        if redo.len() == 0 {
            return Prod { cur, undo, redo, limit };
        }
        let snap = redo.pop().unwrap();
        undo.push(cur);
        let r = Prod { cur: snap, undo, redo, limit };
        proof {
            let a = abs(self);
            assert(a.future[0] == self.redo@.last());
            assert(r.redo@.reverse() =~= a.future.skip(1));
        }
        r
    }
}

} // verus!
