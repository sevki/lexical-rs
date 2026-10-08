//! Markdown block shortcuts as a Lexical plugin component: typing `# `, `> `, `- `,
//! `1. `, `[ ] ` or ```` ``` ```` at the start of a paragraph turns it into that block.

use lexical_plugin::{
    BlockKind, Command, CommandContext, HeadingTag, ListType, Op, Outcome, Plugin, PluginInfo, TextContext,
};

struct MarkdownShortcuts;

/// The characters a shortcut consumes (its marker and the space after it) and the op that
/// applies it.
fn shortcut(text: &str) -> Option<(u32, Op)> {
    // The marker is everything before the first space, except `[ ]`, which has one inside.
    let marker = if text.starts_with("[ ] ") { "[ ]" } else { text.split_once(' ')?.0 };
    let op = match marker {
        "#" => Op::SetBlock(BlockKind::Heading(HeadingTag::H1)),
        "##" => Op::SetBlock(BlockKind::Heading(HeadingTag::H2)),
        "###" => Op::SetBlock(BlockKind::Heading(HeadingTag::H3)),
        "####" => Op::SetBlock(BlockKind::Heading(HeadingTag::H4)),
        "#####" => Op::SetBlock(BlockKind::Heading(HeadingTag::H5)),
        "######" => Op::SetBlock(BlockKind::Heading(HeadingTag::H6)),
        ">" => Op::SetBlock(BlockKind::Quote),
        "```" => Op::SetBlock(BlockKind::Code),
        "-" | "*" => Op::ToggleList(ListType::Bullet),
        "[ ]" => Op::ToggleList(ListType::Check),
        "1." => Op::ToggleList(ListType::Number),
        _ => return None,
    };
    Some((marker.chars().count() as u32 + 1, op))
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
