//! What a plugin is told, and how the operations it answers with are carried out.

use lexical_plugin as w;
use crate::convert::{block_kind, command_from_wit, target_from_wit, Target};
use lexical_core::{Editor, EditorState, NodeKey, NodeType, Point, PointKind, Result};

/// The caret's surroundings, for command handlers.
pub fn command_context(state: &EditorState, editable: bool) -> w::CommandContext {
    let mut ctx = w::CommandContext {
        block: w::BlockKind::Paragraph,
        before_caret: String::new(),
        after_caret: String::new(),
        selected: String::new(),
        editable,
    };
    let Some(sel) = &state.selection else { return ctx };
    let focus = state.inline_point(&sel.focus);
    let Some(block) = state.line_block_of(focus.key) else { return ctx };
    let content = state.block_content(block);
    let at = state.block_offset(&content, &focus);
    ctx.block = block_kind(state, block);
    ctx.before_caret = content.text.chars().take(at).collect();
    ctx.after_caret = content.text.chars().skip(at).collect();
    ctx.selected = state.selected_text();
    ctx
}

/// A text node about to be transformed, or `None` when it is not worth asking plugins.
pub fn text_context(state: &EditorState, key: NodeKey) -> Option<w::TextContext> {
    let text = state.node(key).text()?.to_string();
    let parent = state.parent(key)?;
    let starts_paragraph =
        state.node(parent).node_type() == NodeType::Paragraph && state.node(parent).children.first() == Some(&key);
    let caret = match &state.selection {
        Some(sel) if sel.is_collapsed() && sel.anchor.kind == PointKind::Text && sel.anchor.key == key => {
            Some(sel.anchor.offset as u32)
        }
        _ => None,
    };
    Some(w::TextContext { text, starts_paragraph, caret })
}

/// Carry out a plugin's answer to a text transform, inside the transform's update.
pub fn apply_in_transform(state: &mut EditorState, key: NodeKey, ops: &[w::Op]) -> Result<()> {
    for op in ops {
        match op {
            w::Op::DeletePrefix(n) => delete_prefix(state, key, *n as usize),
            w::Op::InsertText(t) => state.insert_text(t)?,
            w::Op::SetBlock(kind) => apply_target(state, target_from_wit(kind))?,
            w::Op::ToggleList(t) => state.toggle_list(crate::convert::list_from_wit(*t))?,
            // A transform runs inside an update, which cannot start another one.
            w::Op::Dispatch(_) => {}
        }
    }
    Ok(())
}

/// Carry out a plugin's answer to a command, as new updates on `editor`.
pub fn apply_to_editor(editor: &mut Editor, ops: Vec<w::Op>) {
    for op in ops {
        match op {
            // Only meaningful while transforming a text node.
            w::Op::DeletePrefix(_) => {}
            w::Op::InsertText(t) => {
                editor.dispatch(lexical_core::Command::InsertText(t));
            }
            w::Op::SetBlock(kind) => {
                let _ = editor.update(|s| apply_target(s, target_from_wit(&kind)));
            }
            w::Op::ToggleList(t) => {
                editor.dispatch(lexical_core::Command::ToggleList(crate::convert::list_from_wit(t)));
            }
            w::Op::Dispatch(cmd) => {
                editor.dispatch(command_from_wit(cmd));
            }
        }
    }
}

fn apply_target(state: &mut EditorState, target: Target) -> Result<()> {
    match target {
        Target::Block(b) => state.set_block_type(b),
        Target::List(l) => state.toggle_list(l),
    }
}

/// Remove the first `n` characters of text node `key` and put the caret where they were.
fn delete_prefix(state: &mut EditorState, key: NodeKey, n: usize) {
    let Some(text) = state.node(key).text().map(str::to_string) else { return };
    let Some(parent) = state.parent(key) else { return };
    let rest: String = text.chars().skip(n).collect();
    if rest.is_empty() {
        state.remove(key);
        state.set_caret(Point::element(parent, 0));
    } else {
        state.set_text(key, &rest);
        state.set_caret(Point::text(key, 0));
    }
}
