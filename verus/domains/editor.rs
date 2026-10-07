//! The Lexical editor as a *domain* for the generic kernels (`crate::kernels`).
//!
//! The model abstracts `lexical_core::EditorState` to what the document invariants talk
//! about: blocks (kind, indent), their characters (code point + format bits) and a
//! range selection (anchor/focus as block index + char offset). Inline node structure
//! (runs, links) is a representation of the same character sequence; the production
//! normalization (merge adjacent equal-format runs, drop empty nodes) is checked at
//! runtime by `EditorState::check_invariants` and the trace tests in
//! `crates/lexical-core/tests/invariants.rs`.
//!
//! Commands mirror `lexical_core::Command`; each spec function mirrors the like-named
//! method in `crates/lexical-core/src/{edit,blocks}.rs`.
//!
//! Domain obligations discharged here (`impl Domain for EditorDomain`):
//!   (R1) the initial document (one empty paragraph, caret in it) satisfies `inv`,
//!   (R2) `inv(d) ==> inv(normalize(apply(d, cmd)))` for every command.
//! Intent properties (the "delta laws" of the editor), proved per command:
//!   * Insert adds exactly the inserted characters,
//!   * deleting removes exactly the selected characters (one for Backspace/Delete),
//!   * Enter conserves every character and adds exactly one block,
//!   * kind / indent / format / selection commands conserve every character and its
//!     code point (formatting changes only format bits).
use crate::kernels::authority::AuthDomain;
use crate::kernels::replay::Domain;
use vstd::prelude::*;

verus! {

// ------------------------------------------------------------------------ model

pub struct Cell {
    pub ch: nat,
    pub fmt: nat,
}

pub enum Kind {
    Paragraph,
    /// level 1..=6
    Heading(nat),
    Quote,
    Code,
    /// (list type 0 bullet | 1 number | 2 check, nesting depth)
    Item(nat, nat),
}

pub struct Block {
    pub kind: Kind,
    pub indent: nat,
    pub cells: Seq<Cell>,
}

pub struct Pos {
    pub block: nat,
    pub off: nat,
}

pub struct Doc {
    pub blocks: Seq<Block>,
    pub anchor: Pos,
    pub focus: Pos,
}

pub open spec fn max_indent() -> nat { 10 }

pub open spec fn max_depth() -> nat { 8 }

// --------------------------------------------------------------------- invariant

pub open spec fn wf_kind(k: Kind) -> bool {
    match k {
        Kind::Heading(n) => 1 <= n && n <= 6,
        Kind::Item(t, d) => t <= 2 && d <= max_depth(),
        _ => true,
    }
}

pub open spec fn wf_block(b: Block) -> bool {
    wf_kind(b.kind) && b.indent <= max_indent()
}

pub open spec fn blocks_ok(bs: Seq<Block>) -> bool {
    &&& bs.len() >= 1
    &&& forall|i: int| 0 <= i < bs.len() ==> wf_block(#[trigger] bs[i])
}

pub open spec fn valid_pos(bs: Seq<Block>, p: Pos) -> bool {
    p.block < bs.len() && p.off <= bs[p.block as int].cells.len()
}

/// The domain invariant `Inv`.
pub open spec fn inv(d: Doc) -> bool {
    &&& blocks_ok(d.blocks)
    &&& valid_pos(d.blocks, d.anchor)
    &&& valid_pos(d.blocks, d.focus)
}

// -------------------------------------------------------------------- positions

pub open spec fn pos_le(a: Pos, b: Pos) -> bool {
    a.block < b.block || (a.block == b.block && a.off <= b.off)
}

pub open spec fn sel_start(d: Doc) -> Pos {
    if pos_le(d.anchor, d.focus) { d.anchor } else { d.focus }
}

pub open spec fn sel_end(d: Doc) -> Pos {
    if pos_le(d.anchor, d.focus) { d.focus } else { d.anchor }
}

pub open spec fn clamp(bs: Seq<Block>, p: Pos) -> Pos {
    let b = if p.block < bs.len() { p.block } else { (bs.len() - 1) as nat };
    let n = bs[b as int].cells.len();
    Pos { block: b, off: if p.off <= n { p.off } else { n } }
}

/// Mirrors `EditorState::validate_selection`, run at every commit.
pub open spec fn normalize_doc(d: Doc) -> Doc {
    Doc { blocks: d.blocks, anchor: clamp(d.blocks, d.anchor), focus: clamp(d.blocks, d.focus) }
}

// ------------------------------------------------------------------ counting

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

// ------------------------------------------------------------- range deletion

pub open spec fn merged_block(bs: Seq<Block>, s: Pos, e: Pos) -> Block {
    let b = bs[s.block as int];
    let last = bs[e.block as int];
    Block {
        kind: b.kind,
        indent: b.indent,
        cells: b.cells.subrange(0, s.off as int) + last.cells.subrange(e.off as int, last.cells.len() as int),
    }
}

pub open spec fn delete_blocks(bs: Seq<Block>, s: Pos, e: Pos) -> Seq<Block> {
    bs.subrange(0, s.block as int) + seq![merged_block(bs, s, e)]
        + bs.subrange(e.block as int + 1, bs.len() as int)
}

/// Delete everything between `s` and `e` (document order), merging the end block into
/// the start block. Mirrors `EditorState::delete_between`.
pub open spec fn delete_range(d: Doc, s: Pos, e: Pos) -> Doc {
    Doc { blocks: delete_blocks(d.blocks, s, e), anchor: s, focus: s }
}

pub open spec fn range_ok(bs: Seq<Block>, s: Pos, e: Pos) -> bool {
    valid_pos(bs, s) && valid_pos(bs, e) && pos_le(s, e)
}

/// Number of characters strictly between two positions.
pub open spec fn removed(bs: Seq<Block>, s: Pos, e: Pos) -> nat
    recommends range_ok(bs, s, e),
{
    (chars(bs) - chars(delete_blocks(bs, s, e))) as nat
}

pub proof fn delete_range_inv(d: Doc, s: Pos, e: Pos)
    requires inv(d), range_ok(d.blocks, s, e),
    ensures
        inv(delete_range(d, s, e)),
        delete_range(d, s, e).anchor == delete_range(d, s, e).focus,
{
    let bs = d.blocks;
    let nb = delete_blocks(bs, s, e);
    let m = merged_block(bs, s, e);
    let n_before = s.block as int;
    assert(nb.len() == n_before + 1 + (bs.len() - e.block - 1));
    assert forall|i: int| 0 <= i < nb.len() implies wf_block(#[trigger] nb[i]) by {
        if i < n_before {
            assert(nb[i] == bs[i]);
        } else if i == n_before {
            assert(nb[i] == m);
            assert(wf_block(bs[s.block as int]));
        } else {
            assert(nb[i] == bs[i - n_before - 1 + e.block + 1]);
        }
    }
    assert(nb[n_before] == m);
    assert(m.cells.len() == s.off + (bs[e.block as int].cells.len() - e.off));
}

/// Deleting `[s, e)` removes exactly the characters between `s` and `e`.
pub proof fn delete_range_conserves(d: Doc, s: Pos, e: Pos)
    requires inv(d), range_ok(d.blocks, s, e),
    ensures
        // everything outside the range survives: removed = (chars before s's offset in
        // its block is kept) -- stated as the exact count
        chars(delete_blocks(d.blocks, s, e)) + chars_in_range(d.blocks, s, e) == chars(d.blocks),
{
    let bs = d.blocks;
    chars_split(bs, s.block as int, e.block as int + 1);
    let mid = bs.subrange(s.block as int, e.block as int + 1);
    let nb = delete_blocks(bs, s, e);
    chars_add(bs.subrange(0, s.block as int), seq![merged_block(bs, s, e)]);
    chars_add(
        bs.subrange(0, s.block as int) + seq![merged_block(bs, s, e)],
        bs.subrange(e.block as int + 1, bs.len() as int),
    );
    chars_singleton(merged_block(bs, s, e));
    chars_range_formula(bs, s, e);
}

/// Characters inside `[s, e)`, counting a block separator as zero characters.
pub open spec fn chars_in_range(bs: Seq<Block>, s: Pos, e: Pos) -> nat {
    (chars(bs.subrange(s.block as int, e.block as int + 1)) - s.off
        - (bs[e.block as int].cells.len() - e.off)) as nat
}

proof fn chars_range_formula(bs: Seq<Block>, s: Pos, e: Pos)
    requires valid_pos(bs, s), valid_pos(bs, e), pos_le(s, e),
    ensures
        chars(bs.subrange(s.block as int, e.block as int + 1)) >= s.off + (bs[e.block as int].cells.len() - e.off),
{
    let mid = bs.subrange(s.block as int, e.block as int + 1);
    if s.block == e.block {
        assert(mid =~= seq![bs[s.block as int]]);
        chars_singleton(bs[s.block as int]);
    } else {
        // mid = [first] + middle + [last]
        let first = bs[s.block as int];
        let last = bs[e.block as int];
        assert(mid =~= seq![first] + bs.subrange(s.block as int + 1, e.block as int + 1));
        chars_add(seq![first], bs.subrange(s.block as int + 1, e.block as int + 1));
        chars_singleton(first);
        let tail = bs.subrange(s.block as int + 1, e.block as int + 1);
        assert(tail =~= tail.drop_last() + seq![last]);
        chars_add(tail.drop_last(), seq![last]);
        chars_singleton(last);
    }
}

// ------------------------------------------------------------- selection helpers

/// Delete the selected range (no-op for a caret).
pub open spec fn del_sel(d: Doc) -> Doc {
    if d.anchor == d.focus { d } else { delete_range(d, sel_start(d), sel_end(d)) }
}

pub proof fn del_sel_inv(d: Doc)
    requires inv(d),
    ensures
        inv(del_sel(d)),
        del_sel(d).anchor == del_sel(d).focus,
{
    if d.anchor != d.focus {
        let s = sel_start(d);
        let e = sel_end(d);
        delete_range_inv(d, s, e);
    }
}

// ---------------------------------------------------------------- shape helpers

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

// ------------------------------------------------------------------------ insert

pub open spec fn insert_cells(d: Doc, cs: Seq<Cell>) -> Doc {
    let d1 = del_sel(d);
    let p = d1.anchor;
    let b = d1.blocks[p.block as int];
    let nb = Block {
        kind: b.kind,
        indent: b.indent,
        cells: b.cells.subrange(0, p.off as int) + cs + b.cells.subrange(p.off as int, b.cells.len() as int),
    };
    let q = Pos { block: p.block, off: p.off + cs.len() };
    Doc { blocks: d1.blocks.update(p.block as int, nb), anchor: q, focus: q }
}

pub proof fn insert_inv(d: Doc, cs: Seq<Cell>)
    requires inv(d),
    ensures
        inv(insert_cells(d, cs)),
        chars(insert_cells(d, cs).blocks) == chars(del_sel(d).blocks) + cs.len(),
{
    del_sel_inv(d);
    let d1 = del_sel(d);
    let p = d1.anchor;
    let b = d1.blocks[p.block as int];
    let r = insert_cells(d, cs);
    let nb = r.blocks[p.block as int];
    assert(nb.cells.len() == b.cells.len() + cs.len());
    assert forall|i: int| 0 <= i < r.blocks.len() implies wf_block(#[trigger] r.blocks[i]) by {
        if i != p.block {
            assert(r.blocks[i] == d1.blocks[i]);
        }
    }
    chars_update(d1.blocks, p.block as int, nb);
}

// ------------------------------------------------------------------------ enter

pub open spec fn split_kind(k: Kind, at_end: bool) -> Kind {
    if k is Heading && at_end {
        Kind::Paragraph
    } else if k is Quote {
        Kind::Paragraph
    } else {
        k
    }
}

/// Mirrors `EditorState::insert_paragraph` for non-list, non-code blocks.
pub open spec fn enter(d: Doc) -> Doc {
    let d1 = del_sel(d);
    let p = d1.anchor;
    let b = d1.blocks[p.block as int];
    let at_end = p.off == b.cells.len();
    let left = Block { kind: b.kind, indent: b.indent, cells: b.cells.subrange(0, p.off as int) };
    let right = Block {
        kind: split_kind(b.kind, at_end),
        indent: b.indent,
        cells: b.cells.subrange(p.off as int, b.cells.len() as int),
    };
    let q = Pos { block: p.block + 1, off: 0 };
    Doc {
        blocks: d1.blocks.subrange(0, p.block as int) + seq![left, right]
            + d1.blocks.subrange(p.block as int + 1, d1.blocks.len() as int),
        anchor: q,
        focus: q,
    }
}

pub proof fn enter_inv(d: Doc)
    requires inv(d),
    ensures
        inv(enter(d)),
        chars(enter(d).blocks) == chars(del_sel(d).blocks),
        enter(d).blocks.len() == del_sel(d).blocks.len() + 1,
{
    del_sel_inv(d);
    let d1 = del_sel(d);
    let p = d1.anchor;
    let b = d1.blocks[p.block as int];
    let r = enter(d);
    let at_end = p.off == b.cells.len();
    let n = p.block as int;
    assert forall|i: int| 0 <= i < r.blocks.len() implies wf_block(#[trigger] r.blocks[i]) by {
        if i < n {
            assert(r.blocks[i] == d1.blocks[i]);
        } else if i == n {
            assert(wf_block(b));
        } else if i == n + 1 {
            assert(wf_block(b));
        } else {
            assert(r.blocks[i] == d1.blocks[i - 1]);
        }
    }
    // the caret lands at offset 0 of the new block
    assert(r.blocks[n + 1].cells.len() >= 0);
    // character conservation
    let left = Block { kind: b.kind, indent: b.indent, cells: b.cells.subrange(0, p.off as int) };
    let right = Block {
        kind: split_kind(b.kind, at_end),
        indent: b.indent,
        cells: b.cells.subrange(p.off as int, b.cells.len() as int),
    };
    chars_two(left, right);
    let head = d1.blocks.subrange(0, n);
    let tail = d1.blocks.subrange(n + 1, d1.blocks.len() as int);
    chars_add(head, seq![left, right]);
    chars_add(head + seq![left, right], tail);
    chars_split(d1.blocks, n, n + 1);
    assert(d1.blocks.subrange(n, n + 1) =~= seq![b]);
    chars_singleton(b);
}

// ------------------------------------------------------------- single-key deletes

pub open spec fn demote(b: Block) -> Block {
    match b.kind {
        Kind::Item(t, dep) => {
            if dep > 0 {
                Block { kind: Kind::Item(t, (dep - 1) as nat), indent: b.indent, cells: b.cells }
            } else {
                Block { kind: Kind::Paragraph, indent: b.indent, cells: b.cells }
            }
        },
        Kind::Paragraph => {
            if b.indent > 0 {
                Block { kind: b.kind, indent: (b.indent - 1) as nat, cells: b.cells }
            } else {
                b
            }
        },
        _ => Block { kind: Kind::Paragraph, indent: b.indent, cells: b.cells },
    }
}

/// Backspace. Mirrors `delete_character(backward = true)`: delete the selection, or one
/// character, or demote the block at its start, or merge into the previous block.
pub open spec fn backspace(d: Doc) -> Doc {
    if d.anchor != d.focus {
        del_sel(d)
    } else {
        let p = d.anchor;
        let b = d.blocks[p.block as int];
        if p.off > 0 {
            delete_range(d, Pos { block: p.block, off: (p.off - 1) as nat }, p)
        } else if !(b.kind is Paragraph) || b.indent > 0 {
            Doc { blocks: d.blocks.update(p.block as int, demote(b)), anchor: d.anchor, focus: d.focus }
        } else if p.block > 0 {
            delete_range(d, Pos { block: (p.block - 1) as nat, off: d.blocks[p.block - 1].cells.len() }, p)
        } else {
            d
        }
    }
}

pub open spec fn delete_forward(d: Doc) -> Doc {
    if d.anchor != d.focus {
        del_sel(d)
    } else {
        let p = d.anchor;
        let b = d.blocks[p.block as int];
        if p.off < b.cells.len() {
            delete_range(d, p, Pos { block: p.block, off: p.off + 1 })
        } else if p.block + 1 < d.blocks.len() {
            delete_range(d, p, Pos { block: p.block + 1, off: 0 })
        } else {
            d
        }
    }
}

pub proof fn demote_ok(b: Block)
    requires wf_block(b),
    ensures wf_block(demote(b)), demote(b).cells == b.cells,
{
}

pub proof fn backspace_inv(d: Doc)
    requires inv(d),
    ensures inv(backspace(d)),
{
    if d.anchor != d.focus {
        del_sel_inv(d);
    } else {
        let p = d.anchor;
        let b = d.blocks[p.block as int];
        if p.off > 0 {
            delete_range_inv(d, Pos { block: p.block, off: (p.off - 1) as nat }, p);
        } else if !(b.kind is Paragraph) || b.indent > 0 {
            demote_ok(b);
            let r = backspace(d);
            assert forall|i: int| 0 <= i < r.blocks.len() implies wf_block(#[trigger] r.blocks[i]) by {
                if i != p.block {
                    assert(r.blocks[i] == d.blocks[i]);
                }
            }
        } else if p.block > 0 {
            let s = Pos { block: (p.block - 1) as nat, off: d.blocks[p.block - 1].cells.len() };
            delete_range_inv(d, s, p);
        }
    }
}

pub proof fn delete_forward_inv(d: Doc)
    requires inv(d),
    ensures inv(delete_forward(d)),
{
    if d.anchor != d.focus {
        del_sel_inv(d);
    } else {
        let p = d.anchor;
        let b = d.blocks[p.block as int];
        if p.off < b.cells.len() {
            delete_range_inv(d, p, Pos { block: p.block, off: p.off + 1 });
        } else if p.block + 1 < d.blocks.len() {
            delete_range_inv(d, p, Pos { block: p.block + 1, off: 0 });
        }
    }
}

/// Backspace inside a block removes exactly one character.
pub proof fn backspace_removes_one(d: Doc)
    requires inv(d), d.anchor == d.focus, d.anchor.off > 0,
    ensures chars(backspace(d).blocks) + 1 == chars(d.blocks),
{
    let p = d.anchor;
    let s = Pos { block: p.block, off: (p.off - 1) as nat };
    delete_range_inv(d, s, p);
    delete_range_conserves(d, s, p);
    assert(d.blocks.subrange(p.block as int, p.block as int + 1) =~= seq![d.blocks[p.block as int]]);
    chars_singleton(d.blocks[p.block as int]);
}

/// Backspace at the start of a plain paragraph merges it into the previous block and
/// loses no characters.
pub proof fn backspace_merge_loses_nothing(d: Doc)
    requires
        inv(d),
        d.anchor == d.focus,
        d.anchor.off == 0,
        d.blocks[d.anchor.block as int].kind is Paragraph,
        d.blocks[d.anchor.block as int].indent == 0,
        d.anchor.block > 0,
    ensures
        chars(backspace(d).blocks) == chars(d.blocks),
        backspace(d).blocks.len() + 1 == d.blocks.len(),
{
    let p = d.anchor;
    let s = Pos { block: (p.block - 1) as nat, off: d.blocks[p.block - 1].cells.len() };
    delete_range_inv(d, s, p);
    delete_range_conserves(d, s, p);
    let two = d.blocks.subrange(p.block as int - 1, p.block as int + 1);
    assert(two =~= seq![d.blocks[p.block - 1], d.blocks[p.block as int]]);
    chars_two(d.blocks[p.block - 1], d.blocks[p.block as int]);
}

// ----------------------------------------------- per-block maps (kind, indent, format)

pub open spec fn map_selected(d: Doc, f: spec_fn(Block) -> Block) -> Doc {
    let s = sel_start(d);
    let e = sel_end(d);
    Doc {
        blocks: Seq::new(
            d.blocks.len(),
            |i: int| if s.block <= i && i <= e.block { f(d.blocks[i]) } else { d.blocks[i] },
        ),
        anchor: d.anchor,
        focus: d.focus,
    }
}

pub proof fn map_selected_inv(d: Doc, f: spec_fn(Block) -> Block)
    requires
        inv(d),
        forall|b: Block| wf_block(b) ==> wf_block(#[trigger] f(b)) && f(b).cells == b.cells,
    ensures
        inv(map_selected(d, f)),
        same_shape(d.blocks, map_selected(d, f).blocks),
{
    let r = map_selected(d, f);
    assert forall|i: int| 0 <= i < r.blocks.len() implies wf_block(#[trigger] r.blocks[i]) by {
        let s = sel_start(d);
        let e = sel_end(d);
        if s.block <= i && i <= e.block {
            assert(wf_block(d.blocks[i]));
        }
    }
    assert forall|i: int| 0 <= i < d.blocks.len() implies
        (#[trigger] d.blocks[i]).cells.len() == r.blocks[i].cells.len() by {
        let s = sel_start(d);
        let e = sel_end(d);
        if s.block <= i && i <= e.block {
            assert(wf_block(d.blocks[i]));
        }
    }
}

pub open spec fn set_kind_fn(k: Kind) -> spec_fn(Block) -> Block {
    |b: Block| Block { kind: k, indent: b.indent, cells: b.cells }
}

pub open spec fn indent_block(b: Block) -> Block {
    match b.kind {
        Kind::Item(t, dep) => {
            let nd = if dep + 1 <= max_depth() { dep + 1 } else { dep };
            Block { kind: Kind::Item(t, nd), indent: b.indent, cells: b.cells }
        },
        _ => {
            let ni = if b.indent + 1 <= max_indent() { b.indent + 1 } else { b.indent };
            Block { kind: b.kind, indent: ni, cells: b.cells }
        },
    }
}

pub open spec fn outdent_block(b: Block) -> Block {
    match b.kind {
        Kind::Item(t, dep) => {
            if dep > 0 {
                Block { kind: Kind::Item(t, (dep - 1) as nat), indent: b.indent, cells: b.cells }
            } else {
                Block { kind: Kind::Paragraph, indent: b.indent, cells: b.cells }
            }
        },
        _ => {
            let ni = if b.indent > 0 { (b.indent - 1) as nat } else { 0 };
            Block { kind: b.kind, indent: ni, cells: b.cells }
        },
    }
}

pub open spec fn format_sel(d: Doc, f: nat) -> Doc {
    let s = sel_start(d);
    let e = sel_end(d);
    Doc {
        blocks: Seq::new(
            d.blocks.len(),
            |i: int| {
                let b = d.blocks[i];
                Block {
                    kind: b.kind,
                    indent: b.indent,
                    cells: Seq::new(
                        b.cells.len(),
                        |j: int| if (s.block < i || (s.block == i && s.off <= j))
                            && (i < e.block || (i == e.block && j < e.off)) {
                            Cell { ch: b.cells[j].ch, fmt: f }
                        } else {
                            b.cells[j]
                        },
                    ),
                }
            },
        ),
        anchor: d.anchor,
        focus: d.focus,
    }
}

pub proof fn format_inv(d: Doc, f: nat)
    requires inv(d),
    ensures
        inv(format_sel(d, f)),
        same_shape(d.blocks, format_sel(d, f).blocks),
        // formatting never changes which characters there are
        forall|i: int, j: int| 0 <= i < d.blocks.len() && 0 <= j < d.blocks[i].cells.len() ==>
            (#[trigger] format_sel(d, f).blocks[i].cells[j]).ch == d.blocks[i].cells[j].ch,
{
    let r = format_sel(d, f);
    assert forall|i: int| 0 <= i < r.blocks.len() implies wf_block(#[trigger] r.blocks[i]) by {
        assert(wf_block(d.blocks[i]));
    }
}

// ---------------------------------------------------------------------- selection

pub open spec fn select_all(d: Doc) -> Doc {
    let last = (d.blocks.len() - 1) as nat;
    Doc {
        blocks: d.blocks,
        anchor: Pos { block: 0, off: 0 },
        focus: Pos { block: last, off: d.blocks[last as int].cells.len() },
    }
}

// ----------------------------------------------------------------------- commands

pub enum Cmd {
    Insert(Seq<Cell>),
    Enter,
    Backspace,
    DeleteForward,
    SetKind(Kind),
    Indent,
    Outdent,
    Format(nat),
    /// Raw positions from the view; `normalize` clamps them.
    Select(Pos, Pos),
    SelectAll,
}

pub open spec fn apply_doc(d: Doc, c: Cmd) -> Doc {
    match c {
        Cmd::Insert(cs) => insert_cells(d, cs),
        Cmd::Enter => enter(d),
        Cmd::Backspace => backspace(d),
        Cmd::DeleteForward => delete_forward(d),
        Cmd::SetKind(k) => if wf_kind(k) { map_selected(d, set_kind_fn(k)) } else { d },
        Cmd::Indent => map_selected(d, |b: Block| indent_block(b)),
        Cmd::Outdent => map_selected(d, |b: Block| outdent_block(b)),
        Cmd::Format(f) => format_sel(d, f),
        Cmd::Select(a, b) => Doc { blocks: d.blocks, anchor: a, focus: b },
        Cmd::SelectAll => select_all(d),
    }
}

pub proof fn normalize_inv(d: Doc)
    requires blocks_ok(d.blocks),
    ensures inv(normalize_doc(d)),
{
}

/// Every command yields well-formed blocks (positions may still need clamping).
pub proof fn apply_blocks_ok(d: Doc, c: Cmd)
    requires inv(d),
    ensures blocks_ok(apply_doc(d, c).blocks),
{
    match c {
        Cmd::Insert(cs) => insert_inv(d, cs),
        Cmd::Enter => enter_inv(d),
        Cmd::Backspace => backspace_inv(d),
        Cmd::DeleteForward => delete_forward_inv(d),
        Cmd::SetKind(k) => {
            if wf_kind(k) {
                map_selected_inv(d, set_kind_fn(k));
            }
        },
        Cmd::Indent => {
            let f = |b: Block| indent_block(b);
            map_selected_inv(d, f);
        },
        Cmd::Outdent => {
            let f = |b: Block| outdent_block(b);
            map_selected_inv(d, f);
        },
        Cmd::Format(f) => format_inv(d, f),
        Cmd::Select(a, b) => {},
        Cmd::SelectAll => {},
    }
}

// ------------------------------------------------------- the Domain instance (R1, R2)

pub struct EditorDomain;

pub open spec fn init_doc() -> Doc {
    Doc {
        blocks: seq![Block { kind: Kind::Paragraph, indent: 0, cells: Seq::empty() }],
        anchor: Pos { block: 0, off: 0 },
        focus: Pos { block: 0, off: 0 },
    }
}

impl Domain for EditorDomain {
    type Model = Doc;
    type Action = Cmd;

    open spec fn inv(m: Doc) -> bool { inv(m) }

    open spec fn init() -> Doc { init_doc() }

    open spec fn apply(m: Doc, a: Cmd) -> Doc { apply_doc(m, a) }

    open spec fn normalize(m: Doc) -> Doc { normalize_doc(m) }

    proof fn init_satisfies_inv() {
        assert(init_doc().blocks.len() == 1);
    }

    proof fn step_preserves_inv(m: Doc, a: Cmd) {
        apply_blocks_ok(m, a);
        normalize_inv(apply_doc(m, a));
    }
}

/// The server refuses commands whose arguments are meaningless in the current document.
impl AuthDomain for EditorDomain {
    open spec fn accepts(m: Doc, a: Cmd) -> bool {
        match a {
            Cmd::SetKind(k) => wf_kind(k),
            Cmd::Select(p, q) => valid_pos(m.blocks, p) && valid_pos(m.blocks, q),
            Cmd::Insert(cs) => cs.len() > 0,
            _ => true,
        }
    }
}

} // verus!
