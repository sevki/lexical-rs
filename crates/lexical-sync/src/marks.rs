//! Mapping between per-character attributes and CRDT mark key/values.
//!
//! Each attribute is its own key, so concurrent changes to different attributes of the
//! same text (one peer bolds, another links) merge instead of conflicting. Default
//! values are not stored. Decoding is total and bounded: peers are untrusted, so
//! unknown keys and out-of-range values are ignored or clamped.

use crate::flat::{BlockKind, CharAttr, Inline, Line, LINE_END, SOFT_BREAK};
use lexical_core::{Align, HeadingTag, ListType, TextFormat};
use std::collections::BTreeMap;

/// Deepest list nesting accepted from a peer. Rebuilding a document allocates one list
/// per level, so this bounds the work a hostile `depth` mark can cause.
pub const MAX_DEPTH: u32 = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkValue {
    Bool(bool),
    Int(i64),
    Str(String),
}

pub type Marks = BTreeMap<&'static str, MarkValue>;

pub const KEYS: &[&str] = &[
    "bold", "italic", "strike", "underline", "code", "sub", "sup", "highlight", "link", "style", "block",
    "list", "depth", "start", "align", "indent", "checked",
];

const FLAGS: [(TextFormat, &str); 8] = [
    (TextFormat::BOLD, "bold"),
    (TextFormat::ITALIC, "italic"),
    (TextFormat::STRIKETHROUGH, "strike"),
    (TextFormat::UNDERLINE, "underline"),
    (TextFormat::CODE, "code"),
    (TextFormat::SUBSCRIPT, "sub"),
    (TextFormat::SUPERSCRIPT, "sup"),
    (TextFormat::HIGHLIGHT, "highlight"),
];

pub fn encode(attr: &CharAttr) -> Marks {
    let mut m = Marks::new();
    match attr {
        CharAttr::Soft => {}
        CharAttr::Inline(inline) => {
            for (flag, key) in FLAGS {
                if inline.format.contains(flag) {
                    m.insert(key, MarkValue::Bool(true));
                }
            }
            if let Some(url) = &inline.link {
                m.insert("link", MarkValue::Str(url.clone()));
            }
            if !inline.style.is_empty() {
                m.insert("style", MarkValue::Str(inline.style.clone()));
            }
        }
        CharAttr::End(line) => {
            match line.kind {
                BlockKind::Paragraph => {}
                BlockKind::Heading(t) => {
                    m.insert("block", MarkValue::Str(t.as_str().to_string()));
                }
                BlockKind::Quote => {
                    m.insert("block", MarkValue::Str("quote".into()));
                }
                BlockKind::Code => {
                    m.insert("block", MarkValue::Str("code".into()));
                }
                BlockKind::ListItem(t) => {
                    m.insert("list", MarkValue::Str(t.as_str().to_string()));
                    if line.depth > 0 {
                        m.insert("depth", MarkValue::Int(i64::from(line.depth)));
                    }
                    if line.start != 1 {
                        m.insert("start", MarkValue::Int(i64::from(line.start)));
                    }
                    if line.checked == Some(true) {
                        m.insert("checked", MarkValue::Bool(true));
                    }
                }
            }
            if line.align != Align::Start {
                m.insert("align", MarkValue::Str(line.align.as_str().to_string()));
            }
            if line.indent > 0 {
                m.insert("indent", MarkValue::Int(i64::from(line.indent)));
            }
        }
    }
    m
}

/// Rebuild a character's attributes from whatever marks the CRDT holds for it.
pub fn decode(ch: char, get: &dyn Fn(&str) -> Option<MarkValue>) -> CharAttr {
    match ch {
        LINE_END => CharAttr::End(decode_line(get)),
        SOFT_BREAK => CharAttr::Soft,
        _ => CharAttr::Inline(decode_inline(get)),
    }
}

fn flag(get: &dyn Fn(&str) -> Option<MarkValue>, key: &str) -> bool {
    matches!(get(key), Some(MarkValue::Bool(true)))
}

fn string(get: &dyn Fn(&str) -> Option<MarkValue>, key: &str) -> Option<String> {
    match get(key) {
        Some(MarkValue::Str(s)) => Some(s),
        _ => None,
    }
}

fn uint(get: &dyn Fn(&str) -> Option<MarkValue>, key: &str, max: u32) -> u32 {
    match get(key) {
        Some(MarkValue::Int(i)) => i.clamp(0, i64::from(max)) as u32,
        _ => 0,
    }
}

fn decode_inline(get: &dyn Fn(&str) -> Option<MarkValue>) -> Inline {
    let mut format = TextFormat::empty();
    for (f, key) in FLAGS {
        if flag(get, key) {
            format |= f;
        }
    }
    // Sub- and superscript are mutually exclusive in Lexical; keep one deterministically.
    if format.contains(TextFormat::SUBSCRIPT | TextFormat::SUPERSCRIPT) {
        format.remove(TextFormat::SUPERSCRIPT);
    }
    Inline { format, link: string(get, "link").filter(|u| !u.is_empty()), style: string(get, "style").unwrap_or_default() }
}

fn decode_line(get: &dyn Fn(&str) -> Option<MarkValue>) -> Line {
    let list = string(get, "list").and_then(|s| ListType::parse(&s));
    let block = string(get, "block");
    let kind = match (list, block.as_deref()) {
        (Some(t), _) => BlockKind::ListItem(t),
        (None, Some("quote")) => BlockKind::Quote,
        (None, Some("code")) => BlockKind::Code,
        (None, Some(h)) => HeadingTag::parse(h).map_or(BlockKind::Paragraph, BlockKind::Heading),
        (None, None) => BlockKind::Paragraph,
    };
    let is_item = matches!(kind, BlockKind::ListItem(_));
    Line {
        kind,
        depth: if is_item { uint(get, "depth", MAX_DEPTH) } else { 0 },
        align: string(get, "align").map_or(Align::Start, |a| Align::parse(&a)),
        indent: uint(get, "indent", u32::MAX),
        start: if is_item && get("start").is_some() { uint(get, "start", u32::MAX) } else { 1 },
        checked: matches!(kind, BlockKind::ListItem(ListType::Check)).then(|| flag(get, "checked")),
    }
}
