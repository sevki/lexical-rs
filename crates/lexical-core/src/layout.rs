//! Flattened, toolkit-agnostic view of a document: the text a text widget should
//! show, per-line block styling, inline runs, and selection <-> offset mapping.
//! Offsets are in `char`s.

use crate::format::{Align, TextFormat};
use crate::node::*;
use crate::selection::Point;
use crate::state::EditorState;

#[derive(Clone, Debug, PartialEq)]
pub enum BlockStyle {
    Paragraph,
    Heading(HeadingTag),
    Quote,
    Code,
    ListItem { list_type: ListType, ordinal: u32, checked: Option<bool>, depth: u32 },
}

#[derive(Clone, Debug)]
pub struct Line {
    pub key: NodeKey,
    /// Start of the line including any list marker.
    pub start: usize,
    /// Start of editable content (after the marker).
    pub content_start: usize,
    pub end: usize,
    pub style: BlockStyle,
    pub align: Align,
    pub indent: u32,
}

#[derive(Clone, Debug)]
pub struct Run {
    pub key: NodeKey,
    pub start: usize,
    pub end: usize,
    pub format: TextFormat,
    pub link: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub text: String,
    pub lines: Vec<Line>,
    pub runs: Vec<Run>,
}

impl Layout {
    pub fn build(state: &EditorState) -> Layout {
        let mut out = Layout::default();
        let mut pos = 0usize;
        for (i, b) in state.line_blocks().into_iter().enumerate() {
            if i > 0 {
                out.text.push('\n');
                pos += 1;
            }
            let node = state.node(b);
            let style = block_style(state, b);
            let marker = match &style {
                BlockStyle::ListItem { list_type, ordinal, checked, .. } => match list_type {
                    ListType::Bullet => "• ".to_string(),
                    ListType::Number => format!("{ordinal}. "),
                    ListType::Check => {
                        if checked == &Some(true) { "☑ " } else { "☐ " }.to_string()
                    }
                },
                _ => String::new(),
            };
            let start = pos;
            out.text.push_str(&marker);
            let content_start = start + marker.chars().count();
            let content = state.block_content(b);
            out.text.push_str(&content.text);
            for piece in content.pieces.iter().filter(|p| p.is_text) {
                let n = state.node(piece.key);
                let link = state.ancestors(piece.key).into_iter().find_map(|a| match &state.node(a).data {
                    NodeData::Link { url, .. } => Some(url.clone()),
                    _ => None,
                });
                out.runs.push(Run {
                    key: piece.key,
                    start: content_start + piece.start,
                    end: content_start + piece.start + piece.len,
                    format: n.text_format(),
                    link,
                });
            }
            pos = content_start + content.len;
            out.lines.push(Line {
                key: b,
                start,
                content_start,
                end: pos,
                style,
                align: node.align,
                indent: node.indent,
            });
        }
        out
    }

    pub fn char_len(&self) -> usize {
        self.lines.last().map_or(0, |l| l.end)
    }

    /// Buffer offset for a selection point.
    pub fn offset_of(&self, state: &EditorState, p: &Point) -> usize {
        let p = state.inline_point(p);
        let Some(block) = state.line_block_of(p.key) else { return self.char_len() };
        let Some(line) = self.lines.iter().find(|l| l.key == block) else { return self.char_len() };
        let content = state.block_content(block);
        line.content_start + state.block_offset(&content, &p)
    }

    /// Selection point for a buffer offset (clamped out of list markers).
    pub fn point_at(&self, state: &EditorState, offset: usize) -> Point {
        let idx = self.lines.partition_point(|l| l.start <= offset).saturating_sub(1);
        let Some(line) = self.lines.get(idx) else {
            return Point::element(ROOT_KEY, 0);
        };
        let content = state.block_content(line.key);
        let off = offset.saturating_sub(line.content_start).min(content.len);
        state.block_point(line.key, &content, off)
    }

    pub fn line_at(&self, offset: usize) -> Option<&Line> {
        let idx = self.lines.partition_point(|l| l.start <= offset).saturating_sub(1);
        self.lines.get(idx)
    }
}

fn block_style(state: &EditorState, b: NodeKey) -> BlockStyle {
    match &state.node(b).data {
        NodeData::Heading(t) => BlockStyle::Heading(*t),
        NodeData::Quote => BlockStyle::Quote,
        NodeData::Code { .. } => BlockStyle::Code,
        NodeData::ListItem { checked } => {
            let list = state.parent(b);
            let (list_type, start) = match list.map(|l| &state.node(l).data) {
                Some(NodeData::List { list_type, start }) => (*list_type, *start),
                _ => (ListType::Bullet, 1),
            };
            let ordinal = list.map_or(start, |l| {
                let before = state
                    .node(l)
                    .children
                    .iter()
                    .take_while(|&&c| c != b)
                    .filter(|&&c| state.is_line_block(c))
                    .count() as u32;
                start + before
            });
            let depth = state
                .ancestors(b)
                .iter()
                .filter(|&&a| state.node(a).node_type() == NodeType::List)
                .count()
                .saturating_sub(1) as u32;
            BlockStyle::ListItem { list_type, ordinal, checked: *checked, depth }
        }
        _ => BlockStyle::Paragraph,
    }
}
