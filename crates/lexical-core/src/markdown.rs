//! Markdown block shortcuts (`# `, `> `, `- `, `1. `, `[ ] `, ```` ``` ````) as a [`Plugin`].

use crate::blocks::BlockType;
use crate::editor::{Editor, ListenerId, Plugin};
use crate::error::Result;
use crate::node::*;
use crate::selection::Point;
use crate::state::EditorState;

#[derive(Default)]
pub struct MarkdownShortcutsPlugin {
    id: Option<ListenerId>,
}

enum Shortcut {
    Block(BlockType),
    List(ListType),
}

fn match_prefix(text: &str) -> Option<(usize, Shortcut)> {
    let t = |n: usize| HeadingTag::parse(&format!("h{n}")).unwrap();
    for n in 1..=6 {
        if text.starts_with(&format!("{} ", "#".repeat(n))) {
            return Some((n + 1, Shortcut::Block(BlockType::Heading(t(n)))));
        }
    }
    if text.starts_with("> ") {
        return Some((2, Shortcut::Block(BlockType::Quote)));
    }
    if text.starts_with("``` ") {
        return Some((4, Shortcut::Block(BlockType::Code)));
    }
    if text.starts_with("- ") || text.starts_with("* ") {
        return Some((2, Shortcut::List(ListType::Bullet)));
    }
    if text.starts_with("[ ] ") {
        return Some((4, Shortcut::List(ListType::Check)));
    }
    if text.starts_with("1. ") {
        return Some((3, Shortcut::List(ListType::Number)));
    }
    None
}

fn transform(s: &mut EditorState, key: NodeKey) -> Result<()> {
    let Some(text) = s.node(key).text().map(str::to_string) else {
        return Ok(());
    };
    let Some(parent) = s.parent(key) else {
        return Ok(());
    };
    if s.node(parent).node_type() != NodeType::Paragraph
        || s.node(parent).children.first() != Some(&key)
    {
        return Ok(());
    }
    let Some((n, sc)) = match_prefix(&text) else {
        return Ok(());
    };
    // Only fire while typing: the caret must sit right after the prefix.
    match &s.selection {
        Some(sel) if sel.is_collapsed() && sel.anchor == Point::text(key, n) => {}
        _ => return Ok(()),
    }
    let rest: String = text.chars().skip(n).collect();
    if rest.is_empty() {
        s.remove(key);
        s.set_caret(Point::element(parent, 0));
    } else {
        s.set_text(key, &rest);
        s.set_caret(Point::text(key, 0));
    }
    match sc {
        Shortcut::Block(b) => s.set_block_type(b),
        Shortcut::List(l) => s.toggle_list(l),
    }
}

impl Plugin for MarkdownShortcutsPlugin {
    fn set_up(&mut self, editor: &mut Editor) {
        self.id = Some(editor.register_node_transform(NodeType::Text, transform));
    }

    fn tear_down(&mut self, editor: &mut Editor) {
        if let Some(id) = self.id.take() {
            editor.unregister(id);
        }
    }
}
