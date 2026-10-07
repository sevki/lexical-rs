//! Enter, which branches on the block kind exactly like `insert_paragraph`.

use super::blocks::*;
use super::counting::*;
use super::delete::*;
use super::insert::*;
use super::model::*;
use vstd::prelude::*;

verus! {

pub open spec fn split_kind(k: Kind, at_end: bool) -> Kind {
    if k is Heading && at_end {
        Kind::Paragraph
    } else if k is Quote {
        Kind::Paragraph
    } else {
        k
    }
}

/// The plain split of `insert_paragraph`: cut the caret block at the caret (the
/// selection has already been deleted, so `d1` has a collapsed selection).
pub open spec fn split_block(d1: Doc) -> Doc {
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

pub proof fn split_block_inv(d1: Doc)
    requires inv(d1),
    ensures
        inv(split_block(d1)),
        chars(split_block(d1).blocks) == chars(d1.blocks),
        split_block(d1).blocks.len() == d1.blocks.len() + 1,
{
    let p = d1.anchor;
    let b = d1.blocks[p.block as int];
    let r = split_block(d1);
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

pub open spec fn newline_cell() -> Cell {
    Cell { ch: 10, fmt: 0 }
}

/// Double Enter at the end of a code block exits it: the trailing line break goes and an
/// empty paragraph opens after the block.
pub open spec fn is_code_exit(d1: Doc) -> bool {
    let b = d1.blocks[d1.anchor.block as int];
    &&& b.kind is Code
    &&& b.cells.len() > 0
    &&& d1.anchor.off == b.cells.len()
    &&& b.cells.last().ch == 10
}

pub open spec fn exit_code(d1: Doc) -> Doc {
    let p = d1.anchor;
    let b = d1.blocks[p.block as int];
    let code = Block { kind: b.kind, indent: b.indent, cells: b.cells.drop_last() };
    let para = Block { kind: Kind::Paragraph, indent: 0, cells: Seq::empty() };
    let q = Pos { block: p.block + 1, off: 0 };
    Doc {
        blocks: d1.blocks.subrange(0, p.block as int) + seq![code, para]
            + d1.blocks.subrange(p.block as int + 1, d1.blocks.len() as int),
        anchor: q,
        focus: q,
    }
}

/// Mirrors `EditorState::insert_paragraph`, which branches on the block kind:
/// * code block: Enter inserts a line break (double Enter at the end exits the block),
/// * empty list item: Enter outdents it (leaves the list at the top level),
/// * anything else: the block is split at the caret.
pub open spec fn enter(d: Doc) -> Doc {
    let d1 = del_sel(d);
    let p = d1.anchor;
    let b = d1.blocks[p.block as int];
    if b.kind is Code {
        if is_code_exit(d1) { exit_code(d1) } else { insert_cells(d1, seq![newline_cell()]) }
    } else if b.kind is Item && b.cells.len() == 0 {
        Doc { blocks: d1.blocks.update(p.block as int, outdent_block(b)), anchor: d1.anchor, focus: d1.focus }
    } else {
        split_block(d1)
    }
}

pub proof fn exit_code_inv(d1: Doc)
    requires inv(d1), is_code_exit(d1),
    ensures
        inv(exit_code(d1)),
        chars(exit_code(d1).blocks) + 1 == chars(d1.blocks),
        exit_code(d1).blocks.len() == d1.blocks.len() + 1,
{
    let p = d1.anchor;
    let b = d1.blocks[p.block as int];
    let r = exit_code(d1);
    let n = p.block as int;
    let code = Block { kind: b.kind, indent: b.indent, cells: b.cells.drop_last() };
    let para = Block { kind: Kind::Paragraph, indent: 0, cells: Seq::empty() };
    assert forall|i: int| 0 <= i < r.blocks.len() implies wf_block(#[trigger] r.blocks[i]) by {
        if i < n {
            assert(r.blocks[i] == d1.blocks[i]);
        } else if i == n {
            assert(wf_block(b));
        } else if i == n + 1 {
        } else {
            assert(r.blocks[i] == d1.blocks[i - 1]);
        }
    }
    chars_two(code, para);
    let head = d1.blocks.subrange(0, n);
    let tail = d1.blocks.subrange(n + 1, d1.blocks.len() as int);
    chars_add(head, seq![code, para]);
    chars_add(head + seq![code, para], tail);
    chars_split(d1.blocks, n, n + 1);
    assert(d1.blocks.subrange(n, n + 1) =~= seq![b]);
    chars_singleton(b);
}

pub proof fn enter_inv(d: Doc)
    requires inv(d),
    ensures inv(enter(d)),
{
    del_sel_inv(d);
    let d1 = del_sel(d);
    let p = d1.anchor;
    let b = d1.blocks[p.block as int];
    if b.kind is Code {
        if is_code_exit(d1) {
            exit_code_inv(d1);
        } else {
            insert_inv(d1, seq![newline_cell()]);
        }
    } else if b.kind is Item && b.cells.len() == 0 {
        let r = enter(d);
        outdent_ok(b);
        assert forall|i: int| 0 <= i < r.blocks.len() implies wf_block(#[trigger] r.blocks[i]) by {
            if i != p.block {
                assert(r.blocks[i] == d1.blocks[i]);
            }
        }
    } else {
        split_block_inv(d1);
    }
}

} // verus!
