//! Replay kernel (undo / redo / time travel), proved once for every domain.
//! Same architecture as `kernels/Replay.dfy` in metareflection/dafny-replay:
//!
//! ```text
//! Domain (Model, Action, Inv, Init, Apply, Normalize)   <- obligations: R1, R2
//!        |  plugged into
//! Replay kernel: History = { past, present, future }    <- theorems RK1..RK5 (this file)
//!        |  refined by
//! ReplayExec<T>: executable Vec-based history           <- same operations, proven equal
//! ```
//!
//! Domain obligations (the only proofs a domain owes):
//!   (R1) `inv(init())`
//!   (R2) `inv(m) ==> inv(normalize(apply(m, a)))`
//!
//! Kernel theorems (proved here, once):
//!   (RK1) every history reachable by any trace of Do/Undo/Redo satisfies `hist_inv`
//!         (in particular `inv(present)`),
//!   (RK2) Do, Preview, Undo and Redo each preserve `hist_inv`,
//!   (RK3) `Do` leaves no redo branch,
//!   (RK4) Undo and Redo are mutually inverse whenever they act, and `Undo(Do(h,a))`
//!         restores `h.present`/`h.past`,
//!   (RK5) the executable kernel computes exactly the specified transitions, and the
//!         bounded variant keeps `past.len() <= limit` while preserving `hist_inv`.
use vstd::prelude::*;

verus! {

pub trait Domain: Sized {
    type Model;
    type Action;

    spec fn inv(m: Self::Model) -> bool;
    spec fn init() -> Self::Model;
    spec fn apply(m: Self::Model, a: Self::Action) -> Self::Model;
    spec fn normalize(m: Self::Model) -> Self::Model;

    /// (R1)
    proof fn init_satisfies_inv()
        ensures Self::inv(Self::init());

    /// (R2)
    proof fn step_preserves_inv(m: Self::Model, a: Self::Action)
        requires Self::inv(m),
        ensures Self::inv(Self::normalize(Self::apply(m, a)));
}

pub struct History<M> {
    pub past: Seq<M>,
    pub present: M,
    pub future: Seq<M>,
}

pub open spec fn step<D: Domain>(m: D::Model, a: D::Action) -> D::Model {
    D::normalize(D::apply(m, a))
}

pub open spec fn init_history<D: Domain>() -> History<D::Model> {
    History { past: Seq::empty(), present: D::init(), future: Seq::empty() }
}

/// Record the present in the past, apply the action, discard the redo branch.
pub open spec fn do_action<D: Domain>(h: History<D::Model>, a: D::Action) -> History<D::Model> {
    History { past: h.past.push(h.present), present: step::<D>(h.present, a), future: Seq::empty() }
}

/// Apply without recording (live preview while dragging).
pub open spec fn preview<D: Domain>(h: History<D::Model>, a: D::Action) -> History<D::Model> {
    History { past: h.past, present: step::<D>(h.present, a), future: h.future }
}

/// Replace the present and drop redo, but do not add an undo step (typing coalescing).
pub open spec fn do_merge<D: Domain>(h: History<D::Model>, a: D::Action) -> History<D::Model> {
    History { past: h.past, present: step::<D>(h.present, a), future: Seq::empty() }
}

pub open spec fn undo<M>(h: History<M>) -> History<M> {
    if h.past.len() == 0 {
        h
    } else {
        History {
            past: h.past.drop_last(),
            present: h.past.last(),
            future: seq![h.present] + h.future,
        }
    }
}

pub open spec fn redo<M>(h: History<M>) -> History<M> {
    if h.future.len() == 0 {
        h
    } else {
        History { past: h.past.push(h.present), present: h.future[0], future: h.future.skip(1) }
    }
}

pub open spec fn hist_inv<D: Domain>(h: History<D::Model>) -> bool {
    &&& forall|i: int| 0 <= i < h.past.len() ==> D::inv(#[trigger] h.past[i])
    &&& D::inv(h.present)
    &&& forall|j: int| 0 <= j < h.future.len() ==> D::inv(#[trigger] h.future[j])
}

// ------------------------------------------------------------------ lemmas (RK2)

pub proof fn init_history_inv<D: Domain>()
    ensures hist_inv::<D>(init_history::<D>()),
{
    D::init_satisfies_inv();
}

pub proof fn do_preserves<D: Domain>(h: History<D::Model>, a: D::Action)
    requires hist_inv::<D>(h),
    ensures hist_inv::<D>(do_action::<D>(h, a)),
{
    D::step_preserves_inv(h.present, a);
    let h2 = do_action::<D>(h, a);
    assert forall|i: int| 0 <= i < h2.past.len() implies D::inv(#[trigger] h2.past[i]) by {
        if i < h.past.len() {
            assert(h2.past[i] == h.past[i]);
        } else {
            assert(h2.past[i] == h.present);
        }
    }
}

pub proof fn preview_preserves<D: Domain>(h: History<D::Model>, a: D::Action)
    requires hist_inv::<D>(h),
    ensures hist_inv::<D>(preview::<D>(h, a)),
{
    D::step_preserves_inv(h.present, a);
}

pub proof fn do_merge_preserves<D: Domain>(h: History<D::Model>, a: D::Action)
    requires hist_inv::<D>(h),
    ensures hist_inv::<D>(do_merge::<D>(h, a)),
{
    D::step_preserves_inv(h.present, a);
}

pub proof fn undo_preserves<D: Domain>(h: History<D::Model>)
    requires hist_inv::<D>(h),
    ensures hist_inv::<D>(undo(h)),
{
    if h.past.len() > 0 {
        let h2 = undo(h);
        assert forall|i: int| 0 <= i < h2.past.len() implies D::inv(#[trigger] h2.past[i]) by {
            assert(h2.past[i] == h.past[i]);
        }
        assert forall|j: int| 0 <= j < h2.future.len() implies D::inv(#[trigger] h2.future[j]) by {
            if j == 0 {
                assert(h2.future[0] == h.present);
            } else {
                assert(h2.future[j] == h.future[j - 1]);
            }
        }
        assert(D::inv(h.past[h.past.len() - 1]));
    }
}

pub proof fn redo_preserves<D: Domain>(h: History<D::Model>)
    requires hist_inv::<D>(h),
    ensures hist_inv::<D>(redo(h)),
{
    if h.future.len() > 0 {
        let h2 = redo(h);
        assert forall|i: int| 0 <= i < h2.past.len() implies D::inv(#[trigger] h2.past[i]) by {
            if i < h.past.len() {
                assert(h2.past[i] == h.past[i]);
            } else {
                assert(h2.past[i] == h.present);
            }
        }
        assert forall|j: int| 0 <= j < h2.future.len() implies D::inv(#[trigger] h2.future[j]) by {
            assert(h2.future[j] == h.future[j + 1]);
        }
        assert(D::inv(h.future[0]));
    }
}

// ------------------------------------------------------------------ laws (RK3, RK4)

/// (RK3) A new action leaves no redo branch.
pub proof fn do_has_no_redo_branch<D: Domain>(h: History<D::Model>, a: D::Action)
    ensures do_action::<D>(h, a).future.len() == 0,
{
}

/// (RK4) Undo of a Do restores the previous present and past.
pub proof fn undo_do_restores<D: Domain>(h: History<D::Model>, a: D::Action)
    ensures ({
        let u = undo(do_action::<D>(h, a));
        &&& u.present == h.present
        &&& u.past =~= h.past
        &&& u.future =~= seq![step::<D>(h.present, a)]
    }),
{
    let d = do_action::<D>(h, a);
    assert(d.past.len() > 0);
    assert(d.past.drop_last() =~= h.past);
    assert(d.future =~= Seq::<D::Model>::empty());
    assert(seq![d.present] + d.future =~= seq![step::<D>(h.present, a)]);
}

/// (RK4) Redo undoes an Undo, and vice versa.
pub proof fn redo_undo_identity<M>(h: History<M>)
    requires h.past.len() > 0,
    ensures redo(undo(h)) == h,
{
    let u = undo(h);
    assert(u.future.len() > 0);
    assert(u.past.push(u.present) =~= h.past);
    assert(u.future[0] == h.present);
    assert(u.future.skip(1) =~= h.future);
    assert(redo(u).past =~= h.past);
    assert(redo(u).future =~= h.future);
}

pub proof fn undo_redo_identity<M>(h: History<M>)
    requires h.future.len() > 0,
    ensures undo(redo(h)) == h,
{
    let r = redo(h);
    assert(r.past.len() > 0);
    assert(r.past.drop_last() =~= h.past);
    assert(r.past.last() == h.present);
    assert(seq![r.present] + r.future =~= h.future) by {
        assert(r.present == h.future[0]);
        assert(r.future =~= h.future.skip(1));
    }
    assert(undo(r).future =~= h.future);
}

// ---------------------------------------------------------------- traces (RK1)

pub enum Op<A> {
    Do(A),
    Undo,
    Redo,
}

/// The history reached by running a trace of operations from the initial history.
pub open spec fn run<D: Domain>(ops: Seq<Op<D::Action>>) -> History<D::Model>
    decreases ops.len(),
{
    if ops.len() == 0 {
        init_history::<D>()
    } else {
        let h = run::<D>(ops.drop_last());
        match ops.last() {
            Op::Do(a) => do_action::<D>(h, a),
            Op::Undo => undo(h),
            Op::Redo => redo(h),
        }
    }
}

/// (RK1) Whatever the user does, every state in the history satisfies the invariant.
pub proof fn trace_preserves_inv<D: Domain>(ops: Seq<Op<D::Action>>)
    ensures hist_inv::<D>(run::<D>(ops)),
    decreases ops.len(),
{
    if ops.len() == 0 {
        init_history_inv::<D>();
    } else {
        let prefix = ops.drop_last();
        trace_preserves_inv::<D>(prefix);
        let h = run::<D>(prefix);
        match ops.last() {
            Op::Do(a) => do_preserves::<D>(h, a),
            Op::Undo => undo_preserves::<D>(h),
            Op::Redo => redo_preserves::<D>(h),
        }
    }
}

// --------------------------------------------------------- executable kernel (RK5)

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
