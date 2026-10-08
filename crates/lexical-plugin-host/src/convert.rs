//! Translation between the WIT vocabulary and `lexical-core` types.

use lexical_plugin as w;
use lexical_core::{BlockType, Command, EditorState, HeadingTag, ListType, NodeData, NodeKey, TextFormat};

pub fn heading_to_wit(t: HeadingTag) -> w::HeadingTag {
    match t {
        HeadingTag::H1 => w::HeadingTag::H1,
        HeadingTag::H2 => w::HeadingTag::H2,
        HeadingTag::H3 => w::HeadingTag::H3,
        HeadingTag::H4 => w::HeadingTag::H4,
        HeadingTag::H5 => w::HeadingTag::H5,
        HeadingTag::H6 => w::HeadingTag::H6,
    }
}

pub fn heading_from_wit(t: w::HeadingTag) -> HeadingTag {
    match t {
        w::HeadingTag::H1 => HeadingTag::H1,
        w::HeadingTag::H2 => HeadingTag::H2,
        w::HeadingTag::H3 => HeadingTag::H3,
        w::HeadingTag::H4 => HeadingTag::H4,
        w::HeadingTag::H5 => HeadingTag::H5,
        w::HeadingTag::H6 => HeadingTag::H6,
    }
}

pub fn list_to_wit(t: ListType) -> w::ListType {
    match t {
        ListType::Bullet => w::ListType::Bullet,
        ListType::Number => w::ListType::Number,
        ListType::Check => w::ListType::Check,
    }
}

pub fn list_from_wit(t: w::ListType) -> ListType {
    match t {
        w::ListType::Bullet => ListType::Bullet,
        w::ListType::Number => ListType::Number,
        w::ListType::Check => ListType::Check,
    }
}

/// What kind of block `block` is.
pub fn block_kind(state: &EditorState, block: NodeKey) -> w::BlockKind {
    match &state.node(block).data {
        NodeData::Heading(t) => w::BlockKind::Heading(heading_to_wit(*t)),
        NodeData::Quote => w::BlockKind::Quote,
        NodeData::Code { .. } => w::BlockKind::Code,
        NodeData::ListItem { .. } => {
            w::BlockKind::ListItem(list_to_wit(state.list_type_of_item(block).unwrap_or(ListType::Bullet)))
        }
        _ => w::BlockKind::Paragraph,
    }
}

/// The block type to set, or the list to toggle, for a block kind a plugin asked for.
pub enum Target {
    Block(BlockType),
    List(ListType),
}

pub fn target_from_wit(kind: &w::BlockKind) -> Target {
    match kind {
        w::BlockKind::Paragraph => Target::Block(BlockType::Paragraph),
        w::BlockKind::Heading(t) => Target::Block(BlockType::Heading(heading_from_wit(*t))),
        w::BlockKind::Quote => Target::Block(BlockType::Quote),
        w::BlockKind::Code => Target::Block(BlockType::Code),
        w::BlockKind::ListItem(t) => Target::List(list_from_wit(*t)),
    }
}

pub fn format_to_wit(f: TextFormat) -> w::TextFormat {
    let mut out = w::TextFormat::empty();
    for (core, wit) in FORMATS {
        if f.contains(core) {
            out |= wit;
        }
    }
    out
}

pub fn format_from_wit(f: w::TextFormat) -> TextFormat {
    let mut out = TextFormat::empty();
    for (core, wit) in FORMATS {
        if f.contains(wit) {
            out |= core;
        }
    }
    out
}

const FORMATS: [(TextFormat, w::TextFormat); 8] = [
    (TextFormat::BOLD, w::TextFormat::BOLD),
    (TextFormat::ITALIC, w::TextFormat::ITALIC),
    (TextFormat::STRIKETHROUGH, w::TextFormat::STRIKETHROUGH),
    (TextFormat::UNDERLINE, w::TextFormat::UNDERLINE),
    (TextFormat::CODE, w::TextFormat::CODE),
    (TextFormat::SUBSCRIPT, w::TextFormat::SUBSCRIPT),
    (TextFormat::SUPERSCRIPT, w::TextFormat::SUPERSCRIPT),
    (TextFormat::HIGHLIGHT, w::TextFormat::HIGHLIGHT),
];

/// The part of the command vocabulary plugins can see; other commands bypass plugins.
pub fn command_to_wit(cmd: &Command) -> Option<w::Command> {
    Some(match cmd {
        Command::InsertText(t) => w::Command::InsertText(t.clone()),
        Command::Paste(t) => w::Command::Paste(t.clone()),
        Command::InsertParagraph => w::Command::InsertParagraph,
        Command::InsertLineBreak => w::Command::InsertLineBreak,
        Command::DeleteCharacter { backward: true } => w::Command::DeleteBackward,
        Command::DeleteCharacter { backward: false } => w::Command::DeleteForward,
        Command::FormatText(f) => w::Command::FormatText(format_to_wit(*f)),
        Command::ToggleList(t) => w::Command::ToggleList(list_to_wit(*t)),
        Command::Indent => w::Command::Indent,
        Command::Outdent => w::Command::Outdent,
        Command::Undo => w::Command::Undo,
        Command::Redo => w::Command::Redo,
        Command::Custom(name) => w::Command::Custom(name.clone()),
        _ => return None,
    })
}

pub fn command_from_wit(cmd: w::Command) -> Command {
    match cmd {
        w::Command::InsertText(t) => Command::InsertText(t),
        w::Command::Paste(t) => Command::Paste(t),
        w::Command::InsertParagraph => Command::InsertParagraph,
        w::Command::InsertLineBreak => Command::InsertLineBreak,
        w::Command::DeleteBackward => Command::DeleteCharacter { backward: true },
        w::Command::DeleteForward => Command::DeleteCharacter { backward: false },
        w::Command::FormatText(f) => Command::FormatText(format_from_wit(f)),
        w::Command::ToggleList(t) => Command::ToggleList(list_from_wit(t)),
        w::Command::Indent => Command::Indent,
        w::Command::Outdent => Command::Outdent,
        w::Command::Undo => Command::Undo,
        w::Command::Redo => Command::Redo,
        w::Command::Custom(name) => Command::Custom(name),
    }
}
