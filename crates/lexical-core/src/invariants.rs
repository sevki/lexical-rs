//! Runtime check of the document invariants. These are the executable counterpart of
//! `Inv` in the Verus editor domain (`verus/domains/editor/`): every committed state
//! must satisfy them, whatever sequence of commands, undo/redo and loads produced it.

use crate::node::*;
use crate::selection::{Point, PointKind};
use crate::state::EditorState;
use std::collections::HashSet;

impl EditorState {
    /// Returns a description of the first violated invariant.
    pub fn check_invariants(&self) -> Result<(), String> {
        let root = self.get(ROOT_KEY).ok_or("root missing")?;
        if root.parent.is_some() {
            return Err("root has a parent".into());
        }
        if root.children.is_empty() {
            return Err("root has no children".into());
        }
        // Tree shape: every node reachable exactly once, links agree in both directions.
        let mut seen: HashSet<NodeKey> = HashSet::new();
        let mut stack = vec![ROOT_KEY];
        while let Some(k) = stack.pop() {
            if !seen.insert(k) {
                return Err(format!("{k} reachable twice (cycle or shared child)"));
            }
            let n = self.get(k).ok_or(format!("{k} referenced but missing"))?;
            if !n.is_element() && !n.children.is_empty() {
                return Err(format!("leaf {k} has children"));
            }
            for &c in &n.children {
                let cn = self.get(c).ok_or(format!("child {c} of {k} missing"))?;
                if cn.parent != Some(k) {
                    return Err(format!("{c}.parent != {k}"));
                }
                stack.push(c);
            }
            self.check_children_kinds(k)?;
        }
        if seen.len() != self.nodes.len() {
            return Err(format!("{} orphaned node(s)", self.nodes.len() - seen.len()));
        }
        // Normalization: no empty/ adjacent-mergeable text, no empty lists or links.
        for n in self.nodes.values() {
            match &n.data {
                NodeData::Text { text, .. } if text.is_empty() => {
                    return Err(format!("empty text node {}", n.key))
                }
                NodeData::List { .. } | NodeData::Link { .. } if n.children.is_empty() => {
                    return Err(format!("empty {:?} {}", n.node_type(), n.key))
                }
                _ => {}
            }
            for w in n.children.windows(2) {
                if let (
                    NodeData::Text { format: fa, style: sa, mode: TextMode::Normal, .. },
                    NodeData::Text { format: fb, style: sb, mode: TextMode::Normal, .. },
                ) = (&self.node(w[0]).data, &self.node(w[1]).data)
                    && fa == fb && sa == sb {
                        return Err(format!("adjacent mergeable text nodes {} {}", w[0], w[1]));
                    }
            }
        }
        // Selection validity.
        if let Some(sel) = &self.selection {
            for p in [&sel.anchor, &sel.focus] {
                self.check_point(p)?;
            }
        }
        Ok(())
    }

    fn check_point(&self, p: &Point) -> Result<(), String> {
        let n = self.get(p.key).ok_or(format!("selection on missing node {}", p.key))?;
        match p.kind {
            PointKind::Text if !n.is_text() => Err(format!("text point on non-text {}", p.key)),
            PointKind::Text if p.offset > n.text_len() => {
                Err(format!("text offset {} > len {}", p.offset, n.text_len()))
            }
            PointKind::Element if !n.is_element() => Err(format!("element point on leaf {}", p.key)),
            PointKind::Element if p.offset > n.children.len() => {
                Err(format!("element offset {} > {} children", p.offset, n.children.len()))
            }
            _ => Ok(()),
        }
    }

    fn check_children_kinds(&self, k: NodeKey) -> Result<(), String> {
        let n = self.node(k);
        let is_wrapper = |c: NodeKey| {
            self.node(c).node_type() == NodeType::ListItem
                && self.node(c).children.iter().any(|&g| self.node(g).node_type() == NodeType::List)
        };
        for w in n.children.windows(2) {
            match (&self.node(w[0]).data, &self.node(w[1]).data) {
                (NodeData::List { list_type: a, .. }, NodeData::List { list_type: b, .. }) if a == b => {
                    return Err(format!("adjacent {} lists {} {} should be one", a.as_str(), w[0], w[1]));
                }
                (NodeData::ListItem { .. }, NodeData::ListItem { .. }) if is_wrapper(w[0]) && is_wrapper(w[1]) => {
                    return Err(format!("adjacent nesting wrappers {} {} should be one", w[0], w[1]));
                }
                _ => {}
            }
        }
        let kinds = |pred: &dyn Fn(NodeType) -> bool, what: &str| -> Result<(), String> {
            for &c in &n.children {
                let t = self.node(c).node_type();
                if !pred(t) {
                    return Err(format!("{:?} {k} may not contain {t:?} ({what})", n.node_type()));
                }
            }
            Ok(())
        };
        use NodeType::*;
        match n.node_type() {
            Root => kinds(&|t| matches!(t, Paragraph | Heading | Quote | Code | List), "root holds blocks"),
            List => {
                kinds(&|t| t == ListItem, "list holds items")?;
                let is_check = matches!(n.data, NodeData::List { list_type: ListType::Check, .. });
                for &c in &n.children {
                    let item = self.node(c);
                    let wrapper = item.children.iter().any(|&g| self.node(g).node_type() == List);
                    let checked = matches!(item.data, NodeData::ListItem { checked: Some(_) });
                    if checked != (is_check && !wrapper) {
                        return Err(format!(
                            "list item {c} has checked state {checked} inside a {} list",
                            if is_check { "check" } else { "non-check" }
                        ));
                    }
                }
                Ok(())
            }
            ListItem => {
                let nested = n.children.iter().filter(|&&c| self.node(c).node_type() == List).count();
                if nested > 0 && nested != n.children.len() {
                    return Err(format!("list item {k} mixes nested lists and inline content"));
                }
                kinds(&|t| matches!(t, Text | LineBreak | Link | List), "list item content")
            }
            Paragraph | Heading | Quote | Code => {
                kinds(&|t| matches!(t, Text | LineBreak | Link), "block holds inline content")
            }
            Link => kinds(&|t| matches!(t, Text | LineBreak), "link holds text"),
            Text | LineBreak => Ok(()),
        }
    }
}
