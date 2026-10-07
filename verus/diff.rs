//! Verified model of the reconciler's text diff (`reconciler::apply` in
//! `crates/lexical-adw/src/reconciler.rs`): the view is brought in sync with the
//! model by deleting one middle range and inserting one replacement, computed from the
//! common prefix and suffix. Operates on chars (`u32` code points).
//!
//! Proven: splicing `new[p..new_end]` over `old[p..old_end]` yields exactly `new`
//! (so the buffer always ends up equal to the layout text), the edit range is in
//! bounds, and the prefix/suffix are maximal.
use vstd::prelude::*;

verus! {

/// Mirrors `char_prefix_len`.
fn prefix_len(old: &Vec<u32>, new: &Vec<u32>) -> (p: usize)
    ensures
        p <= old.len(), p <= new.len(),
        old@.subrange(0, p as int) =~= new@.subrange(0, p as int),
        p == old.len() || p == new.len() || old@[p as int] != new@[p as int],
{
    let mut i: usize = 0;
    while i < old.len() && i < new.len() && old[i] == new[i]
        invariant
            i <= old.len(), i <= new.len(),
            old@.subrange(0, i as int) =~= new@.subrange(0, i as int),
        decreases old.len() - i,
    {
        proof {
            assert(old@.subrange(0, (i + 1) as int) =~= new@.subrange(0, (i + 1) as int)) by {
                assert forall|k: int| 0 <= k < i + 1 implies old@.subrange(0, (i + 1) as int)[k] == new@.subrange(0, (i + 1) as int)[k] by {
                    if k < i {
                        assert(old@.subrange(0, i as int)[k] == new@.subrange(0, i as int)[k]);
                    }
                }
            }
        }
        i += 1;
    }
    i
}

/// Mirrors `char_suffix_len(old, new, max)`.
fn suffix_len(old: &Vec<u32>, new: &Vec<u32>, max: usize) -> (s: usize)
    requires max <= old.len(), max <= new.len(),
    ensures
        s <= max,
        forall|k: int| 0 <= k < s ==> old@[old.len() - 1 - k] == new@[new.len() - 1 - k],
        s == max || old@[old.len() - 1 - s] != new@[new.len() - 1 - s],
{
    let mut s: usize = 0;
    while s < max && old[old.len() - 1 - s] == new[new.len() - 1 - s]
        invariant
            s <= max, max <= old.len(), max <= new.len(),
            forall|k: int| 0 <= k < s ==> old@[old.len() - 1 - k] == new@[new.len() - 1 - k],
        decreases max - s,
    {
        s += 1;
    }
    s
}

/// `(p, old_end, new_end)`: delete `old[p..old_end]`, insert `new[p..new_end]`.
pub fn minimal_edit(old: &Vec<u32>, new: &Vec<u32>) -> (r: (usize, usize, usize))
    ensures
        r.0 <= r.1 <= old.len(),
        r.0 <= r.2 <= new.len(),
        // Applying the edit reproduces `new` exactly.
        old@.subrange(0, r.0 as int) + new@.subrange(r.0 as int, r.2 as int)
            + old@.subrange(r.1 as int, old.len() as int) =~= new@,
        // Nothing is replaced when the strings are equal.
        old@ =~= new@ ==> r.1 == r.0 && r.2 == r.0,
{
    let ol = old.len();
    let nl = new.len();
    let p = prefix_len(old, new);
    let limit = if ol < nl { ol } else { nl };
    let s = suffix_len(old, new, limit - p);
    let old_end = ol - s;
    let new_end = nl - s;
    proof {
        // p + s <= min(ol, nl)
        assert(old@.subrange(0, p as int) =~= new@.subrange(0, p as int));
        assert(old@.subrange(0, p as int) + new@.subrange(p as int, new_end as int)
            + old@.subrange(old_end as int, ol as int) =~= new@) by {
            let lhs = old@.subrange(0, p as int) + new@.subrange(p as int, new_end as int)
                + old@.subrange(old_end as int, ol as int);
            assert(lhs.len() == nl);
            assert forall|k: int| 0 <= k < nl implies lhs[k] == new@[k] by {
                if k < p {
                    assert(lhs[k] == old@[k]);
                } else if k < new_end {
                    assert(lhs[k] == new@[k]);
                } else {
                    let j = k - new_end;
                    assert(lhs[k] == old@[old_end + j]);
                    // k = nl - 1 - m with m = s - 1 - j  (suffix matches)
                    let m = s as int - 1 - j;
                    assert(0 <= m < s);
                    assert(old@[ol - 1 - m] == new@[nl - 1 - m]);
                    assert(ol - 1 - m == old_end + j);
                    assert(nl - 1 - m == k);
                }
            }
        }
        if old@ =~= new@ {
            // equal strings: prefix covers everything, so the edit is empty
            assert(ol == nl);
            assert(p == ol);
        }
    }
    (p, old_end, new_end)
}

} // verus!
