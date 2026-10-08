//! Text-node edits.

use super::EditorState;
use crate::format::TextFormat;
use crate::node::*;
use crate::selection::PointKind;

impl EditorState {
    pub fn set_text(&mut self, key: NodeKey, new_text: &str) {
        if let NodeData::Text { text, .. } = &mut self.node_mut(key).data {
            *text = new_text.to_string();
        }
    }

    pub fn set_text_format(&mut self, key: NodeKey, f: TextFormat) {
        if let NodeData::Text { format, .. } = &mut self.node_mut(key).data {
            *format = f;
        }
    }

    /// Split a text node at the given char offsets (ascending). Offsets at 0 or the
    /// end are ignored. Returns all resulting pieces; the first keeps `key`.
    /// Selection points move with the characters.
    pub fn split_text(&mut self, key: NodeKey, offsets: &[usize]) -> Vec<NodeKey> {
        let node = self.node(key);
        let NodeData::Text {
            text,
            format,
            style,
            mode,
            detail,
        } = node.data.clone()
        else {
            return vec![key];
        };
        let len = text.chars().count();
        let mut cuts: Vec<usize> = offsets
            .iter()
            .copied()
            .filter(|&o| o > 0 && o < len)
            .collect();
        cuts.dedup();
        if cuts.is_empty() {
            return vec![key];
        }
        let mut bounds = vec![0];
        bounds.extend(&cuts);
        bounds.push(len);
        let chars: Vec<char> = text.chars().collect();
        let piece = |i: usize| -> String { chars[bounds[i]..bounds[i + 1]].iter().collect() };

        self.set_text(key, &piece(0));
        let mut pieces = vec![key];
        let mut prev = key;
        for i in 1..bounds.len() - 1 {
            let n = self.create_node(NodeData::Text {
                text: piece(i),
                format,
                style: style.clone(),
                mode,
                detail,
            });
            self.insert_after(prev, n);
            pieces.push(n);
            prev = n;
        }
        if let Some(sel) = self.selection.as_mut() {
            for p in [&mut sel.anchor, &mut sel.focus] {
                if p.kind == PointKind::Text && p.key == key {
                    let i = (0..pieces.len())
                        .find(|&i| p.offset <= bounds[i + 1])
                        .unwrap_or(pieces.len() - 1);
                    p.key = pieces[i];
                    p.offset -= bounds[i];
                }
            }
        }
        pieces
    }
}
