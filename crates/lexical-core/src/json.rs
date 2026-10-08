//! Lexical-compatible JSON import/export.

use crate::error::{Error, Result};
use crate::format::{Align, TextFormat};
use crate::node::*;
use crate::state::EditorState;
use serde_json::{json, Map, Value};

impl EditorState {
    pub fn to_json(&self) -> Value {
        json!({ "root": self.node_json(ROOT_KEY) })
    }

    pub fn to_json_string(&self) -> String {
        serde_json::to_string_pretty(&self.to_json()).unwrap_or_default()
    }

    fn node_json(&self, key: NodeKey) -> Value {
        let n = self.node(key);
        let mut m = Map::new();
        m.insert("type".into(), n.node_type().as_str().into());
        m.insert("version".into(), 1.into());
        match &n.data {
            NodeData::Text { text, format, style, mode, detail } => {
                m.insert("text".into(), text.clone().into());
                m.insert("format".into(), format.bits().into());
                m.insert("style".into(), style.clone().into());
                m.insert("mode".into(), mode.as_str().into());
                m.insert("detail".into(), (*detail).into());
            }
            NodeData::LineBreak => {}
            d => {
                let kids: Vec<Value> = n.children.iter().map(|&c| self.node_json(c)).collect();
                m.insert("children".into(), kids.into());
                m.insert("direction".into(), "ltr".into());
                m.insert("format".into(), n.align.as_str().into());
                m.insert("indent".into(), n.indent.into());
                if matches!(d, NodeData::Paragraph) {
                    m.insert("textFormat".into(), n.text_format.bits().into());
                    m.insert("textStyle".into(), n.text_style.clone().into());
                }
                match d {
                    NodeData::Heading(t) => {
                        m.insert("tag".into(), t.as_str().into());
                    }
                    NodeData::Code { language } => {
                        m.insert("language".into(), language.clone().map_or(Value::Null, Value::from));
                    }
                    NodeData::List { list_type, start } => {
                        m.insert("listType".into(), list_type.as_str().into());
                        m.insert("start".into(), (*start).into());
                        m.insert("tag".into(), if *list_type == ListType::Number { "ol" } else { "ul" }.into());
                    }
                    NodeData::ListItem { checked } => {
                        // Lexical numbers items from the list's `start`.
                        let start = match n.parent.map(|p| &self.node(p).data) {
                            Some(NodeData::List { start, .. }) => *start,
                            _ => 1,
                        };
                        let value = start + self.index_in_parent(key).unwrap_or(0) as u32;
                        m.insert("value".into(), value.into());
                        if let Some(c) = checked {
                            m.insert("checked".into(), (*c).into());
                        }
                    }
                    NodeData::Link { url, target, rel, title } => {
                        m.insert("url".into(), url.clone().into());
                        m.insert("target".into(), target.clone().map_or(Value::Null, Value::from));
                        m.insert("rel".into(), rel.clone().map_or(Value::Null, Value::from));
                        m.insert("title".into(), title.clone().map_or(Value::Null, Value::from));
                    }
                    _ => {}
                }
            }
        }
        Value::Object(m)
    }

    pub fn from_json_str(s: &str) -> Result<EditorState> {
        let v: Value = serde_json::from_str(s).map_err(|e| Error::InvalidJson(e.to_string()))?;
        Self::from_json(&v)
    }

    pub fn from_json(v: &Value) -> Result<EditorState> {
        let root = v.get("root").ok_or_else(|| Error::InvalidJson("missing root".into()))?;
        if root.get("type").and_then(Value::as_str) != Some("root") {
            return Err(Error::InvalidJson("root.type must be \"root\"".into()));
        }
        let mut s = EditorState::empty();
        for c in root.get("children").and_then(Value::as_array).into_iter().flatten() {
            let k = s.build_node(c)?;
            s.append_child(ROOT_KEY, k);
        }
        if s.root_children().is_empty() {
            let p = s.create_paragraph();
            s.append_child(ROOT_KEY, p);
        }
        // Nothing but blocks at the root: wrap stray inline nodes.
        let first = s.line_blocks().first().copied();
        if let Some(b) = first {
            s.selection = Some(crate::Selection::collapsed(crate::Point::element(b, 0)));
        }
        s.dirty.clear();
        Ok(s)
    }

    fn build_node(&mut self, v: &Value) -> Result<NodeKey> {
        let ty = v.get("type").and_then(Value::as_str).ok_or_else(|| Error::InvalidJson("node without type".into()))?;
        let str_of = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        let data = match ty {
            // `tab` (a text node holding "\t") and `code-highlight` (text inside a code block)
            // are text subclasses in Lexical; here they are plain text.
            "text" | "tab" | "code-highlight" => NodeData::Text {
                text: str_of("text").unwrap_or_default(),
                format: TextFormat::from_bits_truncate(v.get("format").and_then(Value::as_u64).unwrap_or(0) as u32),
                style: str_of("style").unwrap_or_default(),
                mode: TextMode::parse(&str_of("mode").unwrap_or_default()),
                detail: v.get("detail").and_then(Value::as_u64).unwrap_or(0) as u8,
            },
            "linebreak" => NodeData::LineBreak,
            "paragraph" => NodeData::Paragraph,
            "quote" => NodeData::Quote,
            "heading" => NodeData::Heading(
                str_of("tag").and_then(|t| HeadingTag::parse(&t)).unwrap_or(HeadingTag::H1),
            ),
            "code" => NodeData::Code { language: str_of("language") },
            "list" => NodeData::List {
                list_type: str_of("listType").and_then(|t| ListType::parse(&t)).unwrap_or(ListType::Bullet),
                start: v.get("start").and_then(Value::as_u64).unwrap_or(1) as u32,
            },
            "listitem" => NodeData::ListItem { checked: v.get("checked").and_then(Value::as_bool) },
            "link" | "autolink" => NodeData::Link {
                url: str_of("url").unwrap_or_default(),
                target: str_of("target"),
                rel: str_of("rel"),
                title: str_of("title"),
            },
            other => return Err(Error::InvalidJson(format!("unknown node type {other:?}"))),
        };
        let is_el = data.is_element();
        let key = self.create_node(data);
        if is_el {
            let n = self.node_mut(key);
            n.indent = v.get("indent").and_then(Value::as_u64).unwrap_or(0) as u32;
            n.text_format = TextFormat::from_bits_truncate(v.get("textFormat").and_then(Value::as_u64).unwrap_or(0) as u32);
            n.text_style = str_of("textStyle").unwrap_or_default();
            n.align = Align::parse(v.get("format").and_then(Value::as_str).unwrap_or(""));
            for c in v.get("children").and_then(Value::as_array).into_iter().flatten() {
                let ck = self.build_node(c)?;
                self.append_child(key, ck);
            }
        }
        Ok(key)
    }

    /// Plain text with a blank line between top-level blocks (Lexical's `getTextContent`).
    pub fn to_plain_text(&self) -> String {
        self.text_content(ROOT_KEY)
    }
}
