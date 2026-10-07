//! Typing.

use super::counting::*;
use super::delete::*;
use super::model::*;
use vstd::prelude::*;

verus! {

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

} // verus!
