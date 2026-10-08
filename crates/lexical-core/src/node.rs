//! Node model. Every node lives in the [`EditorState`](crate::EditorState) arena and is
//! addressed by a [`NodeKey`]; parent/child links are keys, never references.

use crate::format::{Align, TextFormat};
#[cfg(feature = "jetstream")]
use jetstream_wireformat::JetStreamWireFormat;
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "jetstream", derive(JetStreamWireFormat))]
pub struct NodeKey(pub u64);

pub const ROOT_KEY: NodeKey = NodeKey(0);

impl fmt::Debug for NodeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}
impl fmt::Display for NodeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "jetstream", derive(JetStreamWireFormat))]
pub enum HeadingTag {
    H1,
    H2,
    H3,
    H4,
    H5,
    H6,
}

impl HeadingTag {
    pub fn as_str(self) -> &'static str {
        ["h1", "h2", "h3", "h4", "h5", "h6"][self.level() as usize - 1]
    }
    pub fn level(self) -> u8 {
        self as u8 + 1
    }
    pub fn parse(s: &str) -> Option<HeadingTag> {
        Some(match s {
            "h1" => HeadingTag::H1,
            "h2" => HeadingTag::H2,
            "h3" => HeadingTag::H3,
            "h4" => HeadingTag::H4,
            "h5" => HeadingTag::H5,
            "h6" => HeadingTag::H6,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "jetstream", derive(JetStreamWireFormat))]
pub enum ListType {
    Bullet,
    Number,
    Check,
}

impl ListType {
    pub fn as_str(self) -> &'static str {
        match self {
            ListType::Bullet => "bullet",
            ListType::Number => "number",
            ListType::Check => "check",
        }
    }
    pub fn parse(s: &str) -> Option<ListType> {
        Some(match s {
            "bullet" => ListType::Bullet,
            "number" => ListType::Number,
            "check" => ListType::Check,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "jetstream", derive(JetStreamWireFormat))]
pub enum TextMode {
    #[default]
    Normal,
    Token,
    Segmented,
}

impl TextMode {
    pub fn as_str(self) -> &'static str {
        match self {
            TextMode::Normal => "normal",
            TextMode::Token => "token",
            TextMode::Segmented => "segmented",
        }
    }
    pub fn parse(s: &str) -> TextMode {
        match s {
            "token" => TextMode::Token,
            "segmented" => TextMode::Segmented,
            _ => TextMode::Normal,
        }
    }
}

/// Discriminant of [`NodeData`], used for transforms and serialization.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum NodeType {
    Root,
    Paragraph,
    Heading,
    Quote,
    Code,
    List,
    ListItem,
    Link,
    Text,
    LineBreak,
}

impl NodeType {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeType::Root => "root",
            NodeType::Paragraph => "paragraph",
            NodeType::Heading => "heading",
            NodeType::Quote => "quote",
            NodeType::Code => "code",
            NodeType::List => "list",
            NodeType::ListItem => "listitem",
            NodeType::Link => "link",
            NodeType::Text => "text",
            NodeType::LineBreak => "linebreak",
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
#[cfg_attr(feature = "jetstream", derive(JetStreamWireFormat))]
pub enum NodeData {
    Root,
    Paragraph,
    Heading(HeadingTag),
    Quote,
    Code { language: Option<String> },
    List { list_type: ListType, start: u32 },
    ListItem { checked: Option<bool> },
    Link { url: String, target: Option<String>, rel: Option<String>, title: Option<String> },
    Text {
        // JetStream strings carry a `u16` length; a text node may be longer.
        #[cfg_attr(feature = "jetstream", jetstream(with(crate::wire::wide::Text)))]
        text: String,
        format: TextFormat,
        style: String,
        mode: TextMode,
        detail: u8,
    },
    LineBreak,
}

impl NodeData {
    pub fn node_type(&self) -> NodeType {
        match self {
            NodeData::Root => NodeType::Root,
            NodeData::Paragraph => NodeType::Paragraph,
            NodeData::Heading(_) => NodeType::Heading,
            NodeData::Quote => NodeType::Quote,
            NodeData::Code { .. } => NodeType::Code,
            NodeData::List { .. } => NodeType::List,
            NodeData::ListItem { .. } => NodeType::ListItem,
            NodeData::Link { .. } => NodeType::Link,
            NodeData::Text { .. } => NodeType::Text,
            NodeData::LineBreak => NodeType::LineBreak,
        }
    }

    pub fn text(text: &str, format: TextFormat) -> NodeData {
        NodeData::Text {
            text: text.to_string(),
            format,
            style: String::new(),
            mode: TextMode::Normal,
            detail: 0,
        }
    }

    pub fn is_element(&self) -> bool {
        !matches!(self, NodeData::Text { .. } | NodeData::LineBreak)
    }
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "jetstream", derive(JetStreamWireFormat))]
pub struct Node {
    pub key: NodeKey,
    pub parent: Option<NodeKey>,
    // `u16` element counts would cap a block at 65,535 children.
    #[cfg_attr(feature = "jetstream", jetstream(with(crate::wire::wide::Seq)))]
    pub children: Vec<NodeKey>,
    pub data: NodeData,
    pub indent: u32,
    pub align: Align,
}

impl Node {
    pub fn node_type(&self) -> NodeType {
        self.data.node_type()
    }
    pub fn is_element(&self) -> bool {
        self.data.is_element()
    }
    pub fn is_text(&self) -> bool {
        matches!(self.data, NodeData::Text { .. })
    }
    pub fn is_linebreak(&self) -> bool {
        matches!(self.data, NodeData::LineBreak)
    }
    /// Elements that live inside a line (`Link`) rather than forming their own block.
    pub fn is_inline(&self) -> bool {
        matches!(self.data, NodeData::Link { .. })
    }
    pub fn text(&self) -> Option<&str> {
        match &self.data {
            NodeData::Text { text, .. } => Some(text),
            _ => None,
        }
    }
    pub fn text_len(&self) -> usize {
        self.text().map_or(0, |t| t.chars().count())
    }
    pub fn text_format(&self) -> TextFormat {
        match &self.data {
            NodeData::Text { format, .. } => *format,
            _ => TextFormat::empty(),
        }
    }
    pub fn text_style(&self) -> &str {
        match &self.data {
            NodeData::Text { style, .. } => style,
            _ => "",
        }
    }
}

/// Byte index of the `n`th char of `s` (or `s.len()` when out of range).
pub(crate) fn byte_index(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map_or(s.len(), |(i, _)| i)
}
