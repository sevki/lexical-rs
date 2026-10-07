//! The command set, its semantics, and the `Domain` / `AuthDomain` instances that plug
//! the editor into the generic replay and authority kernels.

use super::blocks::*;
use super::delete::*;
use super::enter::*;
use super::insert::*;
use super::model::*;
use crate::kernels::authority::AuthDomain;
use crate::kernels::replay::Domain;
use vstd::prelude::*;

verus! {

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

pub struct EditorDomain;

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
