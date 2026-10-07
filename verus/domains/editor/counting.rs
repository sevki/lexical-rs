//! Counting characters across blocks; the arithmetic behind every conservation law.

use super::model::*;
use vstd::prelude::*;

verus! {

/// Number of characters in a run of blocks.
pub open spec fn chars(bs: Seq<Block>) -> nat
    decreases bs.len(),
{
    if bs.len() == 0 { 0 } else { chars(bs.drop_last()) + bs.last().cells.len() }
}

pub proof fn chars_add(a: Seq<Block>, b: Seq<Block>)
    ensures chars(a + b) == chars(a) + chars(b),
    decreases b.len(),
{
    if b.len() == 0 {
        assert(a + b =~= a);
    } else {
        chars_add(a, b.drop_last());
        assert((a + b).drop_last() =~= a + b.drop_last());
    }
}

pub proof fn chars_singleton(b: Block)
    ensures chars(seq![b]) == b.cells.len(),
{
    assert(seq![b].drop_last() =~= Seq::<Block>::empty());
    assert(chars(Seq::<Block>::empty()) == 0);
    assert(seq![b].last() == b);
    assert(chars(seq![b]) == chars(seq![b].drop_last()) + seq![b].last().cells.len());
}

pub proof fn chars_split(bs: Seq<Block>, i: int, j: int)
    requires 0 <= i <= j <= bs.len(),
    ensures chars(bs) == chars(bs.subrange(0, i)) + chars(bs.subrange(i, j)) + chars(bs.subrange(j, bs.len() as int)),
{
    assert(bs =~= bs.subrange(0, i) + bs.subrange(i, j) + bs.subrange(j, bs.len() as int));
    chars_add(bs.subrange(0, i), bs.subrange(i, j));
    chars_add(bs.subrange(0, i) + bs.subrange(i, j), bs.subrange(j, bs.len() as int));
}

/// Same number of blocks and the same number of characters in every block.
pub open spec fn same_shape(a: Seq<Block>, b: Seq<Block>) -> bool {
    &&& a.len() == b.len()
    &&& forall|i: int| 0 <= i < a.len() ==> (#[trigger] a[i]).cells.len() == b[i].cells.len()
}

pub proof fn same_shape_chars(a: Seq<Block>, b: Seq<Block>)
    requires same_shape(a, b),
    ensures chars(a) == chars(b),
    decreases a.len(),
{
    if a.len() > 0 {
        assert(same_shape(a.drop_last(), b.drop_last())) by {
            assert forall|i: int| 0 <= i < a.drop_last().len() implies
                (#[trigger] a.drop_last()[i]).cells.len() == b.drop_last()[i].cells.len() by {
                assert(a.drop_last()[i] == a[i]);
                assert(b.drop_last()[i] == b[i]);
            }
        }
        same_shape_chars(a.drop_last(), b.drop_last());
    }
}

pub proof fn chars_update(bs: Seq<Block>, i: int, nb: Block)
    requires 0 <= i < bs.len(),
    ensures chars(bs.update(i, nb)) + bs[i].cells.len() == chars(bs) + nb.cells.len(),
{
    let u = bs.update(i, nb);
    chars_split(bs, i, i + 1);
    chars_split(u, i, i + 1);
    assert(u.subrange(0, i) =~= bs.subrange(0, i));
    assert(u.subrange(i + 1, u.len() as int) =~= bs.subrange(i + 1, bs.len() as int));
    assert(u.subrange(i, i + 1) =~= seq![nb]);
    assert(bs.subrange(i, i + 1) =~= seq![bs[i]]);
    chars_singleton(nb);
    chars_singleton(bs[i]);
}

pub proof fn chars_two(l: Block, r: Block)
    ensures chars(seq![l, r]) == l.cells.len() + r.cells.len(),
{
    assert(seq![l, r] =~= seq![l] + seq![r]);
    chars_add(seq![l], seq![r]);
    chars_singleton(l);
    chars_singleton(r);
}

} // verus!
