//! Binary serialization through the [JetStream](https://jetstream.rs) wire format
//! (`jetstream_wireformat`), enabled by the `jetstream` feature.
//!
//! The node tree is mirrored by [`WireNode`] which derives `JetStreamWireFormat`;
//! [`EditorState`] itself also implements [`WireFormat`], so a document can be used
//! directly as an RPC message payload. Selection is deliberately not part of the
//! wire format (as with Lexical JSON).
//!
//! JetStream encodes strings and vectors with `u16` lengths, so text is split into
//! chunks of at most [`CHUNK`] bytes and an element may hold at most 65535 children;
//! encoding a larger element returns an `InvalidInput` error instead of truncating.

use crate::error::{Error, Result};
use crate::format::{Align, TextFormat};
use crate::node::*;
use crate::state::EditorState;
use jetstream_wireformat::{JetStreamWireFormat, WireFormat};
use std::io::{self, Read, Write};

pub const WIRE_VERSION: u8 = 1;
/// Max bytes per text chunk; comfortably below the `u16` string limit.
const CHUNK: usize = 60_000;
const MAX_DEPTH: usize = 256;

/// Shared element attributes.
#[derive(Clone, Debug, PartialEq, Eq, JetStreamWireFormat)]
pub struct WireBlock {
    pub indent: u32,
    /// 0 start, 1 left, 2 center, 3 right, 4 justify.
    pub align: u8,
}

#[derive(Clone, Debug, PartialEq, JetStreamWireFormat)]
pub enum WireNode {
    Paragraph { block: WireBlock, children: Vec<WireNode> },
    Heading { block: WireBlock, level: u8, children: Vec<WireNode> },
    Quote { block: WireBlock, children: Vec<WireNode> },
    Code { block: WireBlock, language: Option<String>, children: Vec<WireNode> },
    /// `list_type`: 0 bullet, 1 number, 2 check.
    List { block: WireBlock, list_type: u8, start: u32, children: Vec<WireNode> },
    /// `checked`: 0 not a checklist item, 1 unchecked, 2 checked.
    ListItem { block: WireBlock, checked: u8, children: Vec<WireNode> },
    Link {
        block: WireBlock,
        url: String,
        target: Option<String>,
        rel: Option<String>,
        title: Option<String>,
        children: Vec<WireNode>,
    },
    Text { chunks: Vec<String>, format: u32, style: String, mode: u8, detail: u8 },
    LineBreak,
}

#[derive(Clone, Debug, PartialEq, JetStreamWireFormat)]
pub struct WireDocument {
    pub version: u8,
    pub root: WireBlock,
    pub children: Vec<WireNode>,
}

fn align_to_u8(a: Align) -> u8 {
    match a {
        Align::Start => 0,
        Align::Left => 1,
        Align::Center => 2,
        Align::Right => 3,
        Align::Justify => 4,
    }
}

fn align_from_u8(v: u8) -> Align {
    match v {
        1 => Align::Left,
        2 => Align::Center,
        3 => Align::Right,
        4 => Align::Justify,
        _ => Align::Start,
    }
}

fn chunk_text(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    for ch in s.chars() {
        if cur.len() + ch.len_utf8() > CHUNK {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(ch);
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(cur);
    }
    out
}

impl EditorState {
    pub fn to_wire(&self) -> WireDocument {
        let root = self.node(ROOT_KEY);
        WireDocument {
            version: WIRE_VERSION,
            root: WireBlock { indent: root.indent, align: align_to_u8(root.align) },
            children: root.children.iter().map(|&c| self.wire_node(c)).collect(),
        }
    }

    fn wire_node(&self, key: NodeKey) -> WireNode {
        let n = self.node(key);
        let block = WireBlock { indent: n.indent, align: align_to_u8(n.align) };
        let children = || n.children.iter().map(|&c| self.wire_node(c)).collect::<Vec<_>>();
        match &n.data {
            NodeData::Root | NodeData::Paragraph => WireNode::Paragraph { block, children: children() },
            NodeData::Heading(t) => WireNode::Heading { block, level: t.level(), children: children() },
            NodeData::Quote => WireNode::Quote { block, children: children() },
            NodeData::Code { language } => {
                WireNode::Code { block, language: language.clone(), children: children() }
            }
            NodeData::List { list_type, start } => WireNode::List {
                block,
                list_type: match list_type {
                    ListType::Bullet => 0,
                    ListType::Number => 1,
                    ListType::Check => 2,
                },
                start: *start,
                children: children(),
            },
            NodeData::ListItem { checked } => WireNode::ListItem {
                block,
                checked: match checked {
                    None => 0,
                    Some(false) => 1,
                    Some(true) => 2,
                },
                children: children(),
            },
            NodeData::Link { url, target, rel, title } => WireNode::Link {
                block,
                url: url.clone(),
                target: target.clone(),
                rel: rel.clone(),
                title: title.clone(),
                children: children(),
            },
            NodeData::Text { text, format, style, mode, detail } => WireNode::Text {
                chunks: chunk_text(text),
                format: format.bits(),
                style: style.clone(),
                mode: match mode {
                    TextMode::Normal => 0,
                    TextMode::Token => 1,
                    TextMode::Segmented => 2,
                },
                detail: *detail,
            },
            NodeData::LineBreak => WireNode::LineBreak,
        }
    }

    pub fn from_wire(doc: &WireDocument) -> Result<EditorState> {
        if doc.version != WIRE_VERSION {
            return Err(Error::InvalidJson(format!("unsupported wire version {}", doc.version)));
        }
        let mut s = EditorState::empty();
        s.node_mut(ROOT_KEY).indent = doc.root.indent;
        s.node_mut(ROOT_KEY).align = align_from_u8(doc.root.align);
        for c in &doc.children {
            let k = s.build_wire(c, 1)?;
            s.append_child(ROOT_KEY, k);
        }
        if s.root_children().is_empty() {
            let p = s.create_paragraph();
            s.append_child(ROOT_KEY, p);
        }
        if let Some(&b) = s.line_blocks().first() {
            s.selection = Some(crate::Selection::collapsed(crate::Point::element(b, 0)));
        }
        s.dirty.clear();
        Ok(s)
    }

    fn build_wire(&mut self, n: &WireNode, depth: usize) -> Result<NodeKey> {
        if depth > MAX_DEPTH {
            return Err(Error::InvalidJson("document nested too deeply".into()));
        }
        let (data, block, kids): (NodeData, Option<&WireBlock>, &[WireNode]) = match n {
            WireNode::Paragraph { block, children } => (NodeData::Paragraph, Some(block), children),
            WireNode::Heading { block, level, children } => {
                let tag = HeadingTag::parse(&format!("h{level}"))
                    .ok_or_else(|| Error::InvalidJson(format!("bad heading level {level}")))?;
                (NodeData::Heading(tag), Some(block), children)
            }
            WireNode::Quote { block, children } => (NodeData::Quote, Some(block), children),
            WireNode::Code { block, language, children } => {
                (NodeData::Code { language: language.clone() }, Some(block), children)
            }
            WireNode::List { block, list_type, start, children } => {
                let list_type = match list_type {
                    0 => ListType::Bullet,
                    1 => ListType::Number,
                    2 => ListType::Check,
                    v => return Err(Error::InvalidJson(format!("bad list type {v}"))),
                };
                (NodeData::List { list_type, start: *start }, Some(block), children)
            }
            WireNode::ListItem { block, checked, children } => {
                let checked = match checked {
                    0 => None,
                    1 => Some(false),
                    2 => Some(true),
                    v => return Err(Error::InvalidJson(format!("bad checked value {v}"))),
                };
                (NodeData::ListItem { checked }, Some(block), children)
            }
            WireNode::Link { block, url, target, rel, title, children } => (
                NodeData::Link {
                    url: url.clone(),
                    target: target.clone(),
                    rel: rel.clone(),
                    title: title.clone(),
                },
                Some(block),
                children,
            ),
            WireNode::Text { chunks, format, style, mode, detail } => {
                let mode = match mode {
                    1 => TextMode::Token,
                    2 => TextMode::Segmented,
                    _ => TextMode::Normal,
                };
                let data = NodeData::Text {
                    text: chunks.concat(),
                    format: TextFormat::from_bits_truncate(*format),
                    style: style.clone(),
                    mode,
                    detail: *detail,
                };
                return Ok(self.create_node(data));
            }
            WireNode::LineBreak => return Ok(self.create_node(NodeData::LineBreak)),
        };
        let key = self.create_node(data);
        if let Some(b) = block {
            let node = self.node_mut(key);
            node.indent = b.indent;
            node.align = align_from_u8(b.align);
        }
        for c in kids {
            let ck = self.build_wire(c, depth + 1)?;
            self.append_child(key, ck);
        }
        Ok(key)
    }

    pub fn to_wire_bytes(&self) -> io::Result<Vec<u8>> {
        let doc = self.to_wire();
        let mut out = Vec::with_capacity(doc.byte_size() as usize);
        doc.encode(&mut out)?;
        Ok(out)
    }

    pub fn from_wire_bytes(bytes: &[u8]) -> io::Result<EditorState> {
        let mut r = bytes;
        let doc = WireDocument::decode(&mut r)?;
        EditorState::from_wire(&doc).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }
}

/// Lets a whole document travel as a JetStream message.
impl WireFormat for EditorState {
    fn byte_size(&self) -> u32 {
        self.to_wire().byte_size()
    }

    fn encode<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        self.to_wire().encode(writer)
    }

    fn decode<R: Read>(reader: &mut R) -> io::Result<Self> {
        let doc = WireDocument::decode(reader)?;
        EditorState::from_wire(&doc).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }
}
