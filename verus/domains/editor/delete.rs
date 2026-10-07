//! Deleting: selected ranges (with block merging), Backspace and forward Delete.

use super::counting::*;
use super::model::*;
use vstd::prelude::*;

verus! {

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

/// Characters inside `[s, e)`, counting a block separator as zero characters.
pub open spec fn chars_in_range(bs: Seq<Block>, s: Pos, e: Pos) -> nat {
    (chars(bs.subrange(s.block as int, e.block as int + 1)) - s.off
        - (bs[e.block as int].cells.len() - e.off)) as nat
}

/// Deleting `[s, e)` removes exactly the characters between `s` and `e`.
pub proof fn delete_range_conserves(d: Doc, s: Pos, e: Pos)
    requires inv(d), range_ok(d.blocks, s, e),
    ensures
        chars(delete_blocks(d.blocks, s, e)) + chars_in_range(d.blocks, s, e) == chars(d.blocks),
{
    let bs = d.blocks;
    chars_split(bs, s.block as int, e.block as int + 1);
    chars_add(bs.subrange(0, s.block as int), seq![merged_block(bs, s, e)]);
    chars_add(
        bs.subrange(0, s.block as int) + seq![merged_block(bs, s, e)],
        bs.subrange(e.block as int + 1, bs.len() as int),
    );
    chars_singleton(merged_block(bs, s, e));
    chars_range_formula(bs, s, e);
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

/// What Backspace does at the start of a block that is not a plain paragraph.
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

} // verus!
