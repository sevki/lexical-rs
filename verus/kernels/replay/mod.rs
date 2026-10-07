//! Replay kernel (undo / redo / time travel), proved once for every domain.
//! Same architecture as `kernels/Replay.dfy` in metareflection/dafny-replay:
//!
//! ```text
//! Domain (Model, Action, Inv, Init, Apply, Normalize)   <- obligations: R1, R2
//!        |  plugged into
//! Replay kernel: History = { past, present, future }    <- theorems RK1..RK5
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
//!         (in particular `inv(present)`) -- `traces`,
//!   (RK2) Do, Preview, Undo and Redo each preserve `hist_inv` -- `laws`,
//!   (RK3) `Do` leaves no redo branch -- `laws`,
//!   (RK4) Undo and Redo are mutually inverse whenever they act, and `Undo(Do(h,a))`
//!         restores `h.present`/`h.past` -- `laws`,
//!   (RK5) the executable kernel computes exactly the specified transitions, and the
//!         bounded variant keeps `past.len() <= limit` while preserving `hist_inv`
//!         -- `exec`.

mod exec;
mod laws;
mod traces;

pub use exec::*;
pub use laws::*;
pub use traces::*;

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

} // verus!
