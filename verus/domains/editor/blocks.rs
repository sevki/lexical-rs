//! Commands that rewrite block attributes or cell formats without changing text:
//! block kind, indent / outdent, inline format, and selection.

use super::counting::*;
use super::model::*;
use vstd::prelude::*;

verus! {

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

/// List items nest one level deeper; other blocks gain one indent level.
pub open spec fn indent_block(b: Block) -> Block {
    match b.kind {
        Kind::Item(t, dep) => Block { kind: Kind::Item(t, dep + 1), indent: b.indent, cells: b.cells },
        _ => Block { kind: b.kind, indent: b.indent + 1, cells: b.cells },
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

pub proof fn outdent_ok(b: Block)
    requires wf_block(b),
    ensures wf_block(outdent_block(b)), outdent_block(b).cells == b.cells,
{
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

pub open spec fn select_all(d: Doc) -> Doc {
    let last = (d.blocks.len() - 1) as nat;
    Doc {
        blocks: d.blocks,
        anchor: Pos { block: 0, off: 0 },
        focus: Pos { block: last, off: d.blocks[last as int].cells.len() },
    }
}

} // verus!
