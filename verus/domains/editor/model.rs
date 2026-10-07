//! Document model, invariant and positions.

use vstd::prelude::*;

verus! {

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

pub open spec fn wf_kind(k: Kind) -> bool {
    match k {
        Kind::Heading(n) => 1 <= n && n <= 6,
        Kind::Item(t, _) => t <= 2,
        _ => true,
    }
}

pub open spec fn wf_block(b: Block) -> bool {
    wf_kind(b.kind)
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

pub open spec fn init_doc() -> Doc {
    Doc {
        blocks: seq![Block { kind: Kind::Paragraph, indent: 0, cells: Seq::empty() }],
        anchor: Pos { block: 0, off: 0 },
        focus: Pos { block: 0, off: 0 },
    }
}

} // verus!
