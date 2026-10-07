//! (RK5) The executable, `Vec`-based kernel and its proof that it computes exactly the
//! specified transitions.

use super::*;
use vstd::prelude::*;

verus! {

/// Executable history. Models are `Copy` here; the production crate stores cloned
/// `EditorState` snapshots, which is the same stack discipline.
pub struct ReplayExec<T> {
    pub past: Vec<T>,
    pub present: T,
    pub future: Vec<T>,
}

impl<T> View for ReplayExec<T> {
    type V = History<T>;

    open spec fn view(&self) -> History<T> {
        History { past: self.past@, present: self.present, future: self.future@ }
    }
}

impl<T: Copy> ReplayExec<T> {
    pub fn new(init: T) -> (r: Self)
        ensures r@ == (History { past: Seq::<T>::empty(), present: init, future: Seq::<T>::empty() }),
    {
        ReplayExec { past: Vec::new(), present: init, future: Vec::new() }
    }

    /// `do_action` with the next model already computed.
    pub fn record(self, next: T) -> (r: Self)
        ensures
            r@ == (History {
                past: self@.past.push(self@.present),
                present: next,
                future: Seq::<T>::empty(),
            }),
    {
        let ReplayExec { mut past, present, future: _ } = self;
        past.push(present);
        ReplayExec { past, present: next, future: Vec::new() }
    }

    /// Typing coalescing: replace the present, no new undo step.
    pub fn merge(self, next: T) -> (r: Self)
        ensures
            r@ == (History { past: self@.past, present: next, future: Seq::<T>::empty() }),
    {
        let ReplayExec { past, present: _, future: _ } = self;
        ReplayExec { past, present: next, future: Vec::new() }
    }

    pub fn undo(self) -> (r: Self)
        ensures r@ == undo(self@),
    {
        let ReplayExec { mut past, present, mut future } = self;
        if past.len() == 0 {
            return ReplayExec { past, present, future };
        }
        let prev = past.pop().unwrap();
        future.insert(0, present);
        let r = ReplayExec { past, present: prev, future };
        proof {
            assert(r@.future =~= seq![self@.present] + self@.future);
            assert(r@.past =~= self@.past.drop_last());
        }
        r
    }

    pub fn redo(self) -> (r: Self)
        ensures r@ == redo(self@),
    {
        let ReplayExec { mut past, present, mut future } = self;
        if future.len() == 0 {
            return ReplayExec { past, present, future };
        }
        let next = future.remove(0);
        past.push(present);
        let r = ReplayExec { past, present: next, future };
        proof {
            assert(r@.future =~= self@.future.skip(1));
        }
        r
    }

    /// Bounded `record`: the oldest undo step is dropped once `limit` is exceeded.
    pub fn record_bounded(self, next: T, limit: usize) -> (r: Self)
        requires limit > 0, self.past.len() <= limit,
        ensures
            r.past.len() <= limit,
            r@.present == next,
            r@.future.len() == 0,
            self@.past.len() < limit ==> r@.past == self@.past.push(self@.present),
            self@.past.len() == limit ==> r@.past == self@.past.push(self@.present).skip(1),
    {
        let mut r = self.record(next);
        if r.past.len() > limit {
            r.past.remove(0);
        }
        r
    }
}

/// The bounded history keeps only states that were in the unbounded one, so every
/// retained undo step still satisfies the invariant.
pub proof fn bounded_record_preserves<D: Domain>(h: History<D::Model>, next: D::Model, limit: nat)
    requires
        hist_inv::<D>(h),
        D::inv(next),
        limit > 0,
        h.past.len() <= limit,
    ensures ({
        let full = h.past.push(h.present);
        let kept = if full.len() > limit { full.skip(1) } else { full };
        &&& kept.len() <= limit
        &&& forall|i: int| 0 <= i < kept.len() ==> D::inv(#[trigger] kept[i])
    }),
{
    let full = h.past.push(h.present);
    assert forall|i: int| 0 <= i < full.len() implies D::inv(#[trigger] full[i]) by {
        if i < h.past.len() {
            assert(full[i] == h.past[i]);
        }
    }
    if full.len() > limit {
        let kept = full.skip(1);
        assert forall|i: int| 0 <= i < kept.len() implies D::inv(#[trigger] kept[i]) by {
            assert(kept[i] == full[i + 1]);
        }
    }
}

} // verus!
