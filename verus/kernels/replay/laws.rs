//! Invariant preservation (RK2) and the algebraic laws of undo / redo (RK3, RK4).

use super::*;
use vstd::prelude::*;

verus! {

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

} // verus!
