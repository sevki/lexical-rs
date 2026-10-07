//! The interchange model between a Lexical document and a CRDT rich text.
//!
//! A document is flattened to one character sequence in the style of Quill / Peritext:
//!
//! * every block is a *line* whose characters are followed by a terminator `'\n'`;
//! * a soft line break (Lexical `LineBreak`) is U+2028 inside a line;
//! * inline formatting and links are attributes of the characters;
//! * block attributes (heading level, list type and depth, alignment, indent, checked)
//!   are attributes of the terminator.
//!
//! In this form typing, Enter and Backspace are plain inserts and deletes, which is what
//! lets a text CRDT merge concurrent editing without any special block handling.
//! Nested lists are expressed by a `depth` attribute and rebuilt into Lexical's wrapper
//! list items by [`unflatten`](crate::unflatten).

use lexical_core::{Align, EditorState, HeadingTag, ListType, NodeData, Point, TextFormat};

/// Terminates every line.
pub const LINE_END: char = '\n';
/// A soft line break inside a line.
pub const SOFT_BREAK: char = '\u{2028}';

/// Attributes of an ordinary character.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inline {
    pub format: TextFormat,
    pub link: Option<String>,
    /// Lexical's CSS-style string for the text node.
    pub style: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    Heading(HeadingTag),
    Quote,
    Code,
    ListItem(ListType),
}

/// Attributes of a line, stored on its terminator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub kind: BlockKind,
    /// Nesting depth of a list item (0 = top level); 0 for other blocks.
    pub depth: u32,
    pub align: Align,
    pub indent: u32,
    /// `Some` for check-list items only.
    pub checked: Option<bool>,
}

impl Default for Line {
    fn default() -> Self {
        Line { kind: BlockKind::Paragraph, depth: 0, align: Align::Start, indent: 0, checked: None }
    }
}

/// Per-character attributes; the variant is determined by the character itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CharAttr {
    Inline(Inline),
    Soft,
    End(Line),
}

/// A flattened document: parallel character and attribute vectors.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Flat {
    pub chars: Vec<char>,
    pub attrs: Vec<CharAttr>,
}

impl Flat {
    pub fn len(&self) -> usize {
        self.chars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    pub fn push_inline(&mut self, ch: char, inline: Inline) {
        self.chars.push(ch);
        self.attrs.push(CharAttr::Inline(inline));
    }

    pub fn push_soft(&mut self) {
        self.chars.push(SOFT_BREAK);
        self.attrs.push(CharAttr::Soft);
    }

    pub fn push_end(&mut self, line: Line) {
        self.chars.push(LINE_END);
        self.attrs.push(CharAttr::End(line));
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }
}

/// Flatten a document. Literal `'\n'` inside a text node becomes a soft break so the
/// line terminator stays unambiguous.
pub fn flatten(state: &EditorState) -> Flat {
    let mut out = Flat::default();
    for block in state.line_blocks() {
        let content = state.block_content(block);
        for piece in &content.pieces {
            match &state.node(piece.key).data {
                NodeData::Text { text, format, style, .. } => {
                    let link = state.ancestors(piece.key).into_iter().find_map(|a| match &state.node(a).data {
                        NodeData::Link { url, .. } => Some(url.clone()),
                        _ => None,
                    });
                    let inline = Inline { format: *format, link, style: style.clone() };
                    for ch in text.chars() {
                        if ch == LINE_END || ch == SOFT_BREAK {
                            out.push_soft();
                        } else {
                            out.push_inline(ch, inline.clone());
                        }
                    }
                }
                NodeData::LineBreak => out.push_soft(),
                _ => {}
            }
        }
        out.push_end(line_of(state, block));
    }
    out
}

fn line_of(state: &EditorState, block: lexical_core::NodeKey) -> Line {
    let node = state.node(block);
    let (kind, depth, checked) = match &node.data {
        NodeData::Heading(t) => (BlockKind::Heading(*t), 0, None),
        NodeData::Quote => (BlockKind::Quote, 0, None),
        NodeData::Code { .. } => (BlockKind::Code, 0, None),
        NodeData::ListItem { checked } => {
            let ty = state.list_type_of_item(block).unwrap_or(ListType::Bullet);
            (BlockKind::ListItem(ty), state.list_depth(block), (ty == ListType::Check).then(|| checked.unwrap_or(false)))
        }
        _ => (BlockKind::Paragraph, 0, None),
    };
    Line { kind, depth, align: node.align, indent: node.indent, checked }
}

/// Offset in the flat text of a document point.
pub fn flat_offset(state: &EditorState, point: &Point) -> usize {
    let p = state.inline_point(point);
    let target = state.line_block_of(p.key);
    let mut start = 0;
    for block in state.line_blocks() {
        let content = state.block_content(block);
        if Some(block) == target {
            return start + state.block_offset(&content, &p);
        }
        start += content.len + 1;
    }
    start.saturating_sub(1)
}

/// The document point for a flat offset (clamped to the end of the document).
pub fn point_at_flat(state: &EditorState, offset: usize) -> Point {
    let blocks = state.line_blocks();
    let mut start = 0;
    for (i, &block) in blocks.iter().enumerate() {
        let content = state.block_content(block);
        if offset <= start + content.len || i + 1 == blocks.len() {
            return state.block_point(block, &content, offset.saturating_sub(start).min(content.len));
        }
        start += content.len + 1;
    }
    Point::element(lexical_core::ROOT_KEY, 0)
}
