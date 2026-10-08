//! The SDK's plain Rust types, which the runtime-neutral host speaks, to and from the types
//! wasmtime generates for the same WIT interface.

use crate::bindings::lexical::editor::types as g;
use crate::document_bindings::exports::lexical::editor::document_plugin as gd;
use lexical_plugin as sdk;
use lexical_plugin::document as sdkd;

fn heading_to(t: sdk::HeadingTag) -> g::HeadingTag {
    match t {
        sdk::HeadingTag::H1 => g::HeadingTag::H1,
        sdk::HeadingTag::H2 => g::HeadingTag::H2,
        sdk::HeadingTag::H3 => g::HeadingTag::H3,
        sdk::HeadingTag::H4 => g::HeadingTag::H4,
        sdk::HeadingTag::H5 => g::HeadingTag::H5,
        sdk::HeadingTag::H6 => g::HeadingTag::H6,
    }
}

fn heading_from(t: g::HeadingTag) -> sdk::HeadingTag {
    match t {
        g::HeadingTag::H1 => sdk::HeadingTag::H1,
        g::HeadingTag::H2 => sdk::HeadingTag::H2,
        g::HeadingTag::H3 => sdk::HeadingTag::H3,
        g::HeadingTag::H4 => sdk::HeadingTag::H4,
        g::HeadingTag::H5 => sdk::HeadingTag::H5,
        g::HeadingTag::H6 => sdk::HeadingTag::H6,
    }
}

fn list_to(t: sdk::ListType) -> g::ListType {
    match t {
        sdk::ListType::Bullet => g::ListType::Bullet,
        sdk::ListType::Number => g::ListType::Number,
        sdk::ListType::Check => g::ListType::Check,
    }
}

fn list_from(t: g::ListType) -> sdk::ListType {
    match t {
        g::ListType::Bullet => sdk::ListType::Bullet,
        g::ListType::Number => sdk::ListType::Number,
        g::ListType::Check => sdk::ListType::Check,
    }
}

fn block_to(b: &sdk::BlockKind) -> g::BlockKind {
    match b {
        sdk::BlockKind::Paragraph => g::BlockKind::Paragraph,
        sdk::BlockKind::Heading(t) => g::BlockKind::Heading(heading_to(*t)),
        sdk::BlockKind::Quote => g::BlockKind::Quote,
        sdk::BlockKind::Code => g::BlockKind::Code,
        sdk::BlockKind::ListItem(t) => g::BlockKind::ListItem(list_to(*t)),
    }
}

fn block_from(b: &g::BlockKind) -> sdk::BlockKind {
    match b {
        g::BlockKind::Paragraph => sdk::BlockKind::Paragraph,
        g::BlockKind::Heading(t) => sdk::BlockKind::Heading(heading_from(*t)),
        g::BlockKind::Quote => sdk::BlockKind::Quote,
        g::BlockKind::Code => sdk::BlockKind::Code,
        g::BlockKind::ListItem(t) => sdk::BlockKind::ListItem(list_from(*t)),
    }
}

const FORMATS: [(sdk::TextFormat, g::TextFormat); 8] = [
    (sdk::TextFormat::BOLD, g::TextFormat::BOLD),
    (sdk::TextFormat::ITALIC, g::TextFormat::ITALIC),
    (sdk::TextFormat::STRIKETHROUGH, g::TextFormat::STRIKETHROUGH),
    (sdk::TextFormat::UNDERLINE, g::TextFormat::UNDERLINE),
    (sdk::TextFormat::CODE, g::TextFormat::CODE),
    (sdk::TextFormat::SUBSCRIPT, g::TextFormat::SUBSCRIPT),
    (sdk::TextFormat::SUPERSCRIPT, g::TextFormat::SUPERSCRIPT),
    (sdk::TextFormat::HIGHLIGHT, g::TextFormat::HIGHLIGHT),
];

fn format_to(f: sdk::TextFormat) -> g::TextFormat {
    let mut out = g::TextFormat::empty();
    for (s, w) in FORMATS {
        if f.contains(s) {
            out |= w;
        }
    }
    out
}

fn format_from(f: g::TextFormat) -> sdk::TextFormat {
    let mut out = sdk::TextFormat::empty();
    for (s, w) in FORMATS {
        if f.contains(w) {
            out |= s;
        }
    }
    out
}

pub fn command_to(c: &sdk::Command) -> g::Command {
    match c {
        sdk::Command::InsertText(t) => g::Command::InsertText(t.clone()),
        sdk::Command::Paste(t) => g::Command::Paste(t.clone()),
        sdk::Command::InsertParagraph => g::Command::InsertParagraph,
        sdk::Command::InsertLineBreak => g::Command::InsertLineBreak,
        sdk::Command::DeleteBackward => g::Command::DeleteBackward,
        sdk::Command::DeleteForward => g::Command::DeleteForward,
        sdk::Command::FormatText(f) => g::Command::FormatText(format_to(*f)),
        sdk::Command::ToggleList(t) => g::Command::ToggleList(list_to(*t)),
        sdk::Command::Indent => g::Command::Indent,
        sdk::Command::Outdent => g::Command::Outdent,
        sdk::Command::Undo => g::Command::Undo,
        sdk::Command::Redo => g::Command::Redo,
        sdk::Command::Custom(n) => g::Command::Custom(n.clone()),
    }
}

fn command_from(c: g::Command) -> sdk::Command {
    match c {
        g::Command::InsertText(t) => sdk::Command::InsertText(t),
        g::Command::Paste(t) => sdk::Command::Paste(t),
        g::Command::InsertParagraph => sdk::Command::InsertParagraph,
        g::Command::InsertLineBreak => sdk::Command::InsertLineBreak,
        g::Command::DeleteBackward => sdk::Command::DeleteBackward,
        g::Command::DeleteForward => sdk::Command::DeleteForward,
        g::Command::FormatText(f) => sdk::Command::FormatText(format_from(f)),
        g::Command::ToggleList(t) => sdk::Command::ToggleList(list_from(t)),
        g::Command::Indent => sdk::Command::Indent,
        g::Command::Outdent => sdk::Command::Outdent,
        g::Command::Undo => sdk::Command::Undo,
        g::Command::Redo => sdk::Command::Redo,
        g::Command::Custom(n) => sdk::Command::Custom(n),
    }
}

pub fn command_context_to(c: &sdk::CommandContext) -> g::CommandContext {
    g::CommandContext {
        block: block_to(&c.block),
        before_caret: c.before_caret.clone(),
        after_caret: c.after_caret.clone(),
        selected: c.selected.clone(),
        editable: c.editable,
    }
}

pub fn text_context_to(c: &sdk::TextContext) -> g::TextContext {
    g::TextContext { text: c.text.clone(), starts_paragraph: c.starts_paragraph, caret: c.caret }
}

fn op_from(op: g::Op) -> sdk::Op {
    match op {
        g::Op::DeletePrefix(n) => sdk::Op::DeletePrefix(n),
        g::Op::SetBlock(b) => sdk::Op::SetBlock(block_from(&b)),
        g::Op::ToggleList(t) => sdk::Op::ToggleList(list_from(t)),
        g::Op::InsertText(t) => sdk::Op::InsertText(t),
        g::Op::Dispatch(c) => sdk::Op::Dispatch(command_from(c)),
    }
}

pub fn ops_from(ops: Vec<g::Op>) -> Vec<sdk::Op> {
    ops.into_iter().map(op_from).collect()
}

pub fn outcome_from(o: g::Outcome) -> sdk::Outcome {
    sdk::Outcome { handled: o.handled, ops: ops_from(o.ops) }
}

pub fn info_from(i: g::PluginInfo) -> sdk::PluginInfo {
    sdk::PluginInfo { name: i.name, priority: i.priority, transforms_text: i.transforms_text }
}

fn position_to(p: &sdkd::Position) -> gd::Position {
    gd::Position { block: p.block, offset: p.offset }
}

fn position_from(p: &gd::Position) -> sdkd::Position {
    sdkd::Position { block: p.block, offset: p.offset }
}

pub fn selection_to(s: &sdkd::DocumentSelection) -> gd::DocumentSelection {
    gd::DocumentSelection { anchor: position_to(&s.anchor), focus: position_to(&s.focus) }
}

pub fn document_outcome_from(o: gd::DocumentOutcome) -> sdkd::DocumentOutcome {
    sdkd::DocumentOutcome {
        state: o.state,
        selection: o
            .selection
            .map(|s| sdkd::DocumentSelection { anchor: position_from(&s.anchor), focus: position_from(&s.focus) }),
        handled: o.handled,
    }
}
