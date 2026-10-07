//! The kernels instantiated with the Lexical editor domain: what users and the sync
//! protocol are guaranteed. Everything here is a corollary of the generic kernel
//! theorems plus the editor's domain obligations (`domains/editor.rs`).
use crate::domains::editor::*;
use crate::kernels::authority::*;
use crate::kernels::replay::*;
use vstd::prelude::*;

verus! {

/// RK1 for the editor: whatever sequence of commands, undos and redos the user performs,
/// every document in the history is well-formed -- block kinds/indents are in range and
/// the selection always points inside the document.
pub proof fn editor_history_is_always_valid(ops: Seq<Op<Cmd>>)
    ensures hist_inv::<EditorDomain>(run::<EditorDomain>(ops)),
{
    trace_preserves_inv::<EditorDomain>(ops);
}

/// ... and in particular the visible document is valid.
pub proof fn editor_present_is_always_valid(ops: Seq<Op<Cmd>>)
    ensures inv(run::<EditorDomain>(ops).present),
{
    trace_preserves_inv::<EditorDomain>(ops);
}

/// AK1 for the editor: a server applying editor commands from arbitrary, possibly stale
/// or malicious clients always holds a valid document, and that document is exactly the
/// replay of the accepted commands (so the log is a complete record).
pub proof fn editor_server_is_always_valid(reqs: Seq<Request<Cmd>>)
    ensures
        inv(serve::<EditorDomain>(reqs).present),
        serve::<EditorDomain>(reqs).present == replay_log::<EditorDomain>(serve::<EditorDomain>(reqs).log),
        serve::<EditorDomain>(reqs).version == serve::<EditorDomain>(reqs).log.len(),
{
    serve_preserves::<EditorDomain>(reqs);
}

/// Undo restores exactly the document from before the last command (typed text,
/// Enter, Backspace, formatting, ...), and Redo brings it back.
pub proof fn undo_restores_previous_document(h: History<Doc>, c: Cmd)
    ensures ({
        let after = do_action::<EditorDomain>(h, c);
        &&& undo(after).present == h.present
        &&& redo(undo(after)) == after
    }),
{
    undo_do_restores::<EditorDomain>(h, c);
    redo_undo_identity(do_action::<EditorDomain>(h, c));
}

/// Typing a character never destroys other content: it adds exactly one character
/// (beyond whatever the selection replaced).
pub proof fn typing_adds_exactly_the_typed_text(d: Doc, cs: Seq<Cell>)
    requires inv(d),
    ensures chars(insert_cells(d, cs).blocks) == chars(del_sel(d).blocks) + cs.len(),
{
    insert_inv(d, cs);
}

/// A caret-only Insert (no selection) adds exactly `cs.len()` characters.
pub proof fn typing_with_caret_adds_cs(d: Doc, cs: Seq<Cell>)
    requires inv(d), d.anchor == d.focus,
    ensures chars(insert_cells(d, cs).blocks) == chars(d.blocks) + cs.len(),
{
    insert_inv(d, cs);
    assert(del_sel(d) == d);
}

/// Enter in an ordinary block (paragraph, heading, quote, non-empty list item) never
/// loses or duplicates a character and adds exactly one block.
pub proof fn enter_in_plain_block_conserves_text(d: Doc)
    requires
        inv(d),
        d.anchor == d.focus,
        !(d.blocks[d.anchor.block as int].kind is Code),
        !(d.blocks[d.anchor.block as int].kind is Item && d.blocks[d.anchor.block as int].cells.len() == 0),
    ensures
        chars(enter(d).blocks) == chars(d.blocks),
        enter(d).blocks.len() == d.blocks.len() + 1,
{
    assert(del_sel(d) == d);
    split_block_inv(d);
}

/// Enter in a code block inserts exactly one line-break character and no block.
pub proof fn enter_in_code_inserts_a_line_break(d: Doc)
    requires
        inv(d),
        d.anchor == d.focus,
        d.blocks[d.anchor.block as int].kind is Code,
        !is_code_exit(d),
    ensures
        chars(enter(d).blocks) == chars(d.blocks) + 1,
        enter(d).blocks.len() == d.blocks.len(),
{
    assert(del_sel(d) == d);
    insert_inv(d, seq![newline_cell()]);
    assert(seq![newline_cell()].len() == 1);
    let r = insert_cells(d, seq![newline_cell()]);
    assert(r.blocks.len() == d.blocks.len());
}

/// Double Enter at the end of a code block removes only the trailing line break and
/// opens one paragraph.
pub proof fn enter_exits_code_block(d: Doc)
    requires
        inv(d),
        d.anchor == d.focus,
        is_code_exit(d),
    ensures
        chars(enter(d).blocks) + 1 == chars(d.blocks),
        enter(d).blocks.len() == d.blocks.len() + 1,
{
    assert(del_sel(d) == d);
    exit_code_inv(d);
}

/// Enter in an empty list item outdents it and changes no characters or blocks.
pub proof fn enter_on_empty_item_outdents(d: Doc)
    requires
        inv(d),
        d.anchor == d.focus,
        d.blocks[d.anchor.block as int].kind is Item,
        d.blocks[d.anchor.block as int].cells.len() == 0,
    ensures
        chars(enter(d).blocks) == chars(d.blocks),
        enter(d).blocks.len() == d.blocks.len(),
{
    assert(del_sel(d) == d);
    let p = d.anchor;
    let b = d.blocks[p.block as int];
    outdent_ok(b);
    chars_update(d.blocks, p.block as int, outdent_block(b));
}

/// Changing block kind, indenting, outdenting and formatting never add, remove or alter
/// characters; formatting changes only format bits.
pub proof fn block_and_format_commands_conserve_text(d: Doc, c: Cmd)
    requires
        inv(d),
        c is SetKind || c is Indent || c is Outdent || c is Format,
    ensures
        same_shape(d.blocks, apply_doc(d, c).blocks),
        chars(apply_doc(d, c).blocks) == chars(d.blocks),
{
    match c {
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
        _ => {},
    }
    same_shape_chars(d.blocks, apply_doc(d, c).blocks);
}

/// Moving the selection never touches the document content.
pub proof fn selection_commands_do_not_edit(d: Doc, a: Pos, b: Pos)
    ensures normalize_doc(apply_doc(d, Cmd::Select(a, b))).blocks == d.blocks,
{
}

/// The sync-side guarantee behind optimistic editing: whatever the user sees (server
/// base plus pending local commands) is a valid document.
pub proof fn optimistic_view_is_valid(c: Client<EditorDomain>)
    requires inv(c.base),
    ensures inv(client_model::<EditorDomain>(c)),
{
    reapply_preserves::<EditorDomain>(c.base, c.pending);
}

} // verus!
