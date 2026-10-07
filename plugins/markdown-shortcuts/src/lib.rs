//! Markdown block shortcuts as a Lexical plugin component: typing `# `, `> `, `- `,
//! `1. `, `[ ] ` or ```` ``` ```` at the start of a paragraph turns it into that block.

use lexical_plugin::{
    BlockKind, Command, CommandContext, HeadingTag, ListType, Op, Outcome, Plugin, PluginInfo, TextContext,
};

struct MarkdownShortcuts;

const HEADINGS: [HeadingTag; 6] =
    [HeadingTag::H1, HeadingTag::H2, HeadingTag::H3, HeadingTag::H4, HeadingTag::H5, HeadingTag::H6];

/// The characters a shortcut consumes and the op that applies it.
fn shortcut(text: &str) -> Option<(u32, Op)> {
    for (i, tag) in HEADINGS.iter().enumerate() {
        let n = i + 1;
        if text.starts_with(&format!("{} ", "#".repeat(n))) {
            return Some((n as u32 + 1, Op::SetBlock(BlockKind::Heading(*tag))));
        }
    }
    if text.starts_with("> ") {
        return Some((2, Op::SetBlock(BlockKind::Quote)));
    }
    if text.starts_with("``` ") {
        return Some((4, Op::SetBlock(BlockKind::Code)));
    }
    if text.starts_with("- ") || text.starts_with("* ") {
        return Some((2, Op::ToggleList(ListType::Bullet)));
    }
    if text.starts_with("[ ] ") {
        return Some((4, Op::ToggleList(ListType::Check)));
    }
    if text.starts_with("1. ") {
        return Some((3, Op::ToggleList(ListType::Number)));
    }
    None
}

impl Plugin for MarkdownShortcuts {
    fn info() -> PluginInfo {
        PluginInfo { name: "markdown-shortcuts".into(), priority: 0, transforms_text: true }
    }

    fn handle_command(_cmd: Command, _ctx: CommandContext) -> Outcome {
        Outcome { handled: false, ops: vec![] }
    }

    fn transform_text(ctx: TextContext) -> Vec<Op> {
        // Only while typing: the caret must sit right after the prefix.
        let (true, Some(caret)) = (ctx.starts_paragraph, ctx.caret) else { return vec![] };
        match shortcut(&ctx.text) {
            Some((n, op)) if n == caret => vec![Op::DeletePrefix(n), op],
            _ => vec![],
        }
    }
}

lexical_plugin::export_plugin!(MarkdownShortcuts);
