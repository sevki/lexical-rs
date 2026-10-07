//! Verified model of the selection bookkeeping in `EditorState::split_text`
//! (`crates/lexical-core/src/state.rs`): after a text node is cut into pieces, a
//! selection offset must land in exactly one piece, at the same character.
//!
//! The Rust code does
//! `(0..pieces.len()).find(|&i| p.offset <= bounds[i + 1])` and then
//! `p.key = pieces[i]; p.offset -= bounds[i]`.
//!
//! Proven (for any strictly increasing `bounds` that starts at 0):
//!  * the chosen piece contains the offset: `bounds[i] <= offset <= bounds[i+1]`,
//!  * the relative offset is `offset - bounds[i]`, so `bounds[i] + rel == offset`
//!    (no character is lost or duplicated),
//!  * first-fit: an offset sitting exactly on a cut maps to the end of the earlier piece,
//!  * piece lengths sum to the original length (`bounds.last()`).
use vstd::prelude::*;

verus! {

pub open spec fn bounds_wf(b: Seq<usize>) -> bool {
    &&& b.len() >= 2
    &&& b[0] == 0
    &&& forall|i: int, j: int| 0 <= i < j < b.len() ==> b[i] < b[j]
}

/// Mirrors the `find` in `split_text`. Returns `(piece, relative_offset)`.
pub fn locate(bounds: &Vec<usize>, offset: usize) -> (r: (usize, usize))
    requires
        bounds_wf(bounds@),
        offset <= bounds@[bounds@.len() - 1],
    ensures
        r.0 + 1 < bounds.len(),
        bounds@[r.0 as int] <= offset <= bounds@[r.0 as int + 1],
        r.1 == offset - bounds@[r.0 as int],
        bounds@[r.0 as int] + r.1 == offset,
        // first-fit: earlier piece wins on a shared boundary
        r.0 == 0 || bounds@[r.0 as int] < offset,
{
    let n = bounds.len();
    let mut i: usize = 0;
    while i + 2 < n && offset > bounds[i + 1]
        invariant
            i + 1 < n, n == bounds.len(),
            bounds_wf(bounds@),
            offset <= bounds@[n - 1],
            bounds@[i as int] <= offset || i == 0,
            // every earlier piece ended strictly before `offset`
            forall|k: int| 0 <= k < i ==> #[trigger] bounds@[k + 1] < offset,
        decreases n - i,
    {
        i += 1;
    }
    // Either we stopped on the first piece whose end reaches `offset`, or on the last piece.
    proof {
        if i + 2 >= n {
            assert(bounds@[i as int + 1] == bounds@[n - 1]) by {
                if i + 1 < n - 1 { assert(false); }
            }
        }
        if i > 0 {
            let k = i as int - 1;
            assert(bounds@[k + 1] < offset);
        }
    }
    let rel = offset - bounds[i];
    (i, rel)
}

/// Piece lengths sum to the node length (so splitting never changes the text).
pub proof fn lemma_pieces_sum(b: Seq<usize>, upto: int)
    requires
        bounds_wf(b),
        0 <= upto < b.len(),
    ensures
        sum_lens(b, upto) == b[upto] - b[0],
    decreases upto,
{
    if upto > 0 {
        lemma_pieces_sum(b, upto - 1);
    }
}

/// Sum of piece lengths `b[1]-b[0] + ... + b[upto]-b[upto-1]`.
pub open spec fn sum_lens(b: Seq<usize>, upto: int) -> int
    decreases upto,
{
    if upto <= 0 { 0 } else { sum_lens(b, upto - 1) + (b[upto] - b[upto - 1]) }
}

} // verus!
