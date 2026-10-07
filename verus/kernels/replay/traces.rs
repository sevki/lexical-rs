//! (RK1) Whatever the user does, the invariant holds: induction over operation traces.

use super::*;
use vstd::prelude::*;

verus! {

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

/// (RK1) Every state in the history satisfies the invariant after any trace.
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

} // verus!
