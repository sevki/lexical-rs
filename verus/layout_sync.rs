//! Verified model of the model<->view selection sync in `lexical_core::layout::Layout`
//! (`offset_of` / `point_at`), at line granularity.
//!
//! Each line `i` occupies `[start(i), start(i) + marker_i + len_i]` in the text widget
//! (`marker_i` is the list marker prefix, `len_i` the editable content) and lines are
//! separated by one `\n`, so `start(i+1) = start(i) + marker_i + len_i + 1`.
//!
//! Proven:
//!  * `start` is strictly increasing,
//!  * `offset_of` stays inside its line and never lands in the next one,
//!  * **round trip**: `point_at(offset_of(line, off)) == (line, off)` for every valid
//!    point, so pushing the engine selection to the widget and reading it back is lossless,
//!  * `point_at` always returns a valid point (offset within content length) and
//!    clamps positions inside a list marker to the start of the content.
use vstd::prelude::*;

verus! {

pub struct Line {
    pub marker: usize,
    pub len: usize,
}

pub open spec fn start(ls: Seq<Line>, i: int) -> int
    decreases i,
{
    if i <= 0 { 0 } else { start(ls, i - 1) + ls[i - 1].marker + ls[i - 1].len + 1 }
}

pub open spec fn content_start(ls: Seq<Line>, i: int) -> int {
    start(ls, i) + ls[i].marker
}

pub proof fn lemma_start_strict(ls: Seq<Line>, i: int, j: int)
    requires 0 <= i < j <= ls.len(),
    ensures start(ls, i) < start(ls, j),
    decreases j - i,
{
    if j - i > 1 {
        lemma_start_strict(ls, i, j - 1);
    }
    assert(start(ls, j) == start(ls, j - 1) + ls[j - 1].marker + ls[j - 1].len + 1);
}

/// Mirrors `Layout::offset_of` once the point is resolved to `(line, off)`.
pub fn offset_of(ls: &Vec<Line>, starts: &Vec<usize>, line: usize, off: usize) -> (r: usize)
    requires
        line < ls.len(),
        off <= ls@[line as int].len,
        starts.len() == ls.len(),
        forall|k: int| 0 <= k < ls.len() ==> starts@[k] == start(ls@, k),
        start(ls@, ls.len() as int) < usize::MAX,
    ensures
        r as int == content_start(ls@, line as int) + off,
{
    proof {
        lemma_start_strict(ls@, line as int, ls.len() as int);
    }
    starts[line] + ls[line].marker + off
}

/// Mirrors `Layout::point_at`: last line with `start <= offset`, then clamp into content.
pub fn point_at(ls: &Vec<Line>, starts: &Vec<usize>, offset: usize) -> (r: (usize, usize))
    requires
        ls.len() > 0,
        starts.len() == ls.len(),
        forall|k: int| 0 <= k < ls.len() ==> starts@[k] == start(ls@, k),
        start(ls@, ls.len() as int) < usize::MAX,
    ensures
        r.0 < ls.len(),
        r.1 <= ls@[r.0 as int].len,
        // chosen line is the last one starting at or before `offset` (or the first line)
        r.0 == 0 || starts@[r.0 as int] <= offset,
        r.0 + 1 == ls.len() || offset < starts@[r.0 as int + 1] || (r.0 == 0 && offset < starts@[0]),
        // offsets inside the marker clamp to the start of content
        offset <= content_start(ls@, r.0 as int) ==> r.1 == 0,
        // otherwise the offset is preserved exactly (up to the line end)
        offset > content_start(ls@, r.0 as int)
            && offset <= content_start(ls@, r.0 as int) + ls@[r.0 as int].len
            ==> r.1 as int == offset - content_start(ls@, r.0 as int),
{
    let n = ls.len();
    let mut i: usize = 0;
    // partition_point(|l| l.start <= offset) - 1, saturating
    while i + 1 < n && starts[i + 1] <= offset
        invariant
            i < n, n == ls.len(), starts.len() == n,
            forall|k: int| 0 <= k < n ==> starts@[k] == start(ls@, k),
            i == 0 || starts@[i as int] <= offset,
        decreases n - i,
    {
        i += 1;
    }
    let cs = starts[i] + ls[i].marker;
    let rel = if offset > cs { offset - cs } else { 0 };
    let len = ls[i].len;
    let rel = if rel > len { len } else { rel };
    (i, rel)
}

/// The round trip theorem: widget offset -> engine point is the inverse of
/// engine point -> widget offset.
pub fn roundtrip(ls: &Vec<Line>, starts: &Vec<usize>, line: usize, off: usize)
    requires
        line < ls.len(),
        off <= ls@[line as int].len,
        starts.len() == ls.len(),
        forall|k: int| 0 <= k < ls.len() ==> starts@[k] == start(ls@, k),
        start(ls@, ls.len() as int) < usize::MAX,
{
    let x = offset_of(ls, starts, line, off);
    let (l2, o2) = point_at(ls, starts, x);
    proof {
        let n = ls.len() as int;
        let cs = content_start(ls@, line as int);
        lemma_start_strict(ls@, line as int, n);
        // x lies within line `line`: start(line) <= x <= start(line)+marker+len < start(line+1)
        if (line as int) + 1 < n {
            assert(start(ls@, line as int + 1) == start(ls@, line as int) + ls@[line as int].marker
                + ls@[line as int].len + 1);
            assert(x < starts@[line as int + 1]);
        }
        // So the last line starting <= x is `line`: any later line starts after x.
        if l2 != line {
            if l2 > line {
                lemma_start_strict(ls@, line as int + 1, l2 as int + 1);
                assert(starts@[l2 as int] > x) by {
                    if (l2 as int) > (line as int) + 1 {
                        lemma_start_strict(ls@, line as int + 1, l2 as int);
                    }
                }
                assert(false);
            } else {
                // l2 < line: then the line after l2 starts at or before x, contradicting maximality
                assert(l2 as int + 1 < n);
                lemma_start_strict(ls@, l2 as int + 1, line as int + 1);
                assert(starts@[l2 as int + 1] <= x);
                assert(false);
            }
        }
        if off == 0 {
            assert(o2 == 0);
        } else {
            assert(x as int > cs);
            assert(o2 as int == x as int - cs);
        }
    }
    assert(l2 == line);
    assert(o2 == off);
}

} // verus!
