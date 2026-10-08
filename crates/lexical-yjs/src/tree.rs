//! The `@lexical/yjs` tree as plain data, and its conversion to and from Lexical JSON.

use crate::{Error, Result};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use yrs::types::text::YChange;
use yrs::types::xml::Xml;
use yrs::{Any, Map as _, MapRef, Number, Out, ReadTxn, Text, XmlTextRef};

pub type Props = BTreeMap<String, Any>;

pub enum Child {
    Elem(Elem),
    Run(Run),
    Leaf(Leaf),
}

/// An element: an embedded `XmlText` whose attributes are `props`.
pub struct Elem {
    pub props: Props,
    pub children: Vec<Child>,
    pub handle: Option<XmlTextRef>,
}

/// A text node: an embedded `Map` of `props` followed by `text` in the parent.
pub struct Run {
    pub props: Props,
    pub text: String,
    pub handle: Option<MapRef>,
}

/// A node without text or children (line break, decorator): an embedded `Map`.
pub struct Leaf {
    pub props: Props,
}

pub fn utf16_len(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}

impl Child {
    /// Length in Yjs units: an embed counts one, text counts its UTF-16 units.
    pub fn len(&self) -> u32 {
        match self {
            Child::Run(r) => 1 + utf16_len(&r.text),
            _ => 1,
        }
    }
    pub fn type_name(&self) -> &str {
        let props = match self {
            Child::Elem(e) => &e.props,
            Child::Run(r) => &r.props,
            Child::Leaf(l) => &l.props,
        };
        props.get("__type").and_then(as_str).unwrap_or("")
    }
}

fn as_str(a: &Any) -> Option<&str> {
    match a {
        Any::String(s) => Some(s),
        _ => None,
    }
}

pub fn num(n: f64) -> Any {
    Any::Number(Number::try_i64(n))
}

pub fn as_f64(a: &Any) -> Option<f64> {
    match a {
        Any::Number(n) => n.as_f64(),
        _ => None,
    }
}

// ---- Lexical JSON <-> Any ----

pub fn value_to_any(v: &Value) -> Any {
    Any::from_json(&v.to_string()).unwrap_or(Any::Null)
}

pub fn any_to_value(a: &Any) -> Value {
    match a {
        Any::Undefined => Value::Null,
        Any::Number(n) if n.as_i64().is_some() => Value::from(n.as_i64().unwrap_or(0)),
        other => {
            let mut s = String::new();
            other.to_json(&mut s);
            serde_json::from_str(&s).unwrap_or(Value::Null)
        }
    }
}

// Lexical's alignment is a number in Yjs and a string in JSON.
const ALIGN: [&str; 7] = ["", "left", "center", "right", "justify", "start", "end"];
const MODE: [&str; 3] = ["normal", "token", "segmented"];

fn index_of(table: &[&str], s: &str) -> f64 {
    // "start" is the default alignment, which JSON writes as "" and Lexical stores as 0.
    if s == "start" {
        return 0.0;
    }
    table.iter().position(|t| *t == s).unwrap_or(0) as f64
}

fn is_text_type(t: &str) -> bool {
    matches!(t, "text" | "tab" | "code-highlight")
}

// ---- JSON -> tree (the state we want) ----

pub fn from_json(v: &Value) -> Child {
    let ty = v.get("type").and_then(Value::as_str).unwrap_or("");
    let obj = v.as_object().cloned().unwrap_or_default();
    let mut props = Props::new();
    props.insert("__type".into(), Any::String(ty.into()));
    if is_text_type(ty) {
        let get = |k: &str| obj.get(k).cloned().unwrap_or(Value::Null);
        props.insert("__format".into(), value_to_any(&get("format")));
        props.insert("__style".into(), Any::String(get("style").as_str().unwrap_or("").into()));
        let mode = get("mode");
        props.insert("__mode".into(), num(index_of(&MODE, mode.as_str().unwrap_or("normal"))));
        props.insert("__detail".into(), value_to_any(&get("detail")));
        let text = get("text").as_str().unwrap_or("").to_string();
        return Child::Run(Run { props, text, handle: None });
    }
    let is_elem = obj.get("children").is_some_and(Value::is_array);
    for (k, val) in &obj {
        match k.as_str() {
            "type" | "version" | "children" | "direction" => {}
            "format" if is_elem => {
                props.insert("__format".into(), num(index_of(&ALIGN, val.as_str().unwrap_or(""))));
            }
            _ => {
                props.insert(format!("__{k}"), value_to_any(val));
            }
        }
    }
    if !is_elem {
        return Child::Leaf(Leaf { props });
    }
    // Defaults Lexical writes for every element.
    props.entry("__style".into()).or_insert(Any::String("".into()));
    props.entry("__dir".into()).or_insert(Any::Null);
    props.entry("__textFormat".into()).or_insert(num(0.0));
    props.entry("__textStyle".into()).or_insert(Any::String("".into()));
    props.entry("__indent".into()).or_insert(num(0.0));
    let children = obj
        .get("children")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(from_json)
        .collect();
    Child::Elem(Elem { props, children, handle: None })
}

// ---- tree -> JSON ----

pub fn to_json(c: &Child) -> Value {
    match c {
        Child::Run(r) => {
            let g = |k: &str| r.props.get(k).map(any_to_value).unwrap_or(Value::Null);
            let mode = r.props.get("__mode").and_then(as_f64).unwrap_or(0.0) as usize;
            json!({
                "type": r.props.get("__type").and_then(as_str).unwrap_or("text"),
                "version": 1,
                "text": r.text,
                "format": g("__format"),
                "style": r.props.get("__style").and_then(as_str).unwrap_or(""),
                "mode": MODE.get(mode).copied().unwrap_or("normal"),
                "detail": g("__detail"),
            })
        }
        Child::Leaf(l) => {
            let mut m = props_to_object(&l.props, false);
            m.insert("version".into(), 1.into());
            Value::Object(m)
        }
        Child::Elem(e) => {
            let mut m = props_to_object(&e.props, true);
            m.insert("version".into(), 1.into());
            m.insert("direction".into(), Value::Null);
            m.insert("children".into(), e.children.iter().map(to_json).collect());
            Value::Object(m)
        }
    }
}

fn props_to_object(props: &Props, element: bool) -> Map<String, Value> {
    let mut m = Map::new();
    for (k, v) in props {
        let Some(name) = k.strip_prefix("__") else { continue };
        match name {
            "dir" => {}
            "style" if element => {}
            "format" if element => {
                let n = as_f64(v).unwrap_or(0.0) as usize;
                // 5 ("start") is the default alignment, written "" in JSON.
                let s = if n == 5 { "" } else { ALIGN.get(n).copied().unwrap_or("") };
                m.insert("format".into(), s.into());
            }
            _ => {
                m.insert(name.into(), any_to_value(v));
            }
        }
    }
    m
}

// ---- Yjs -> tree (the state we have) ----

fn out_to_any(o: Out) -> Any {
    match o {
        Out::Any(a) => a,
        _ => Any::Null,
    }
}

fn map_props<T: ReadTxn>(txn: &T, m: &MapRef) -> Props {
    m.iter(txn).map(|(k, v)| (k.to_string(), out_to_any(v))).collect()
}

/// The children of an element (or of the root) in order, with handles for editing.
pub fn read_children<T: ReadTxn, X: Text>(txn: &T, parent: &X) -> Vec<Child> {
    let mut out: Vec<Child> = vec![];
    for d in parent.diff(txn, |_: YChange| ()) {
        match d.insert {
            Out::Any(Any::String(s)) => match out.last_mut() {
                Some(Child::Run(r)) => r.text.push_str(&s),
                // Characters with no node in front of them: a plain text node.
                _ => {
                    let mut props = Props::new();
                    props.insert("__type".into(), Any::String("text".into()));
                    props.insert("__format".into(), num(0.0));
                    props.insert("__style".into(), Any::String("".into()));
                    props.insert("__mode".into(), num(0.0));
                    props.insert("__detail".into(), num(0.0));
                    out.push(Child::Run(Run { props, text: s.to_string(), handle: None }));
                }
            },
            Out::YMap(m) => {
                let props = map_props(txn, &m);
                let ty = props.get("__type").and_then(as_str).unwrap_or("");
                if is_text_type(ty) {
                    out.push(Child::Run(Run { props, text: String::new(), handle: Some(m) }));
                } else {
                    out.push(Child::Leaf(Leaf { props }));
                }
            }
            Out::YXmlText(x) => {
                let props = x.attributes(txn).map(|(k, v)| (k.to_string(), out_to_any(v))).collect();
                let children = read_children(txn, &x);
                out.push(Child::Elem(Elem { props, children, handle: Some(x) }));
            }
            _ => {}
        }
    }
    out
}

/// The whole document as Lexical JSON.
pub fn read_json<T: ReadTxn, X: Text>(txn: &T, root: &X) -> Result<Value> {
    let children = read_children(txn, root);
    if children.iter().any(|c| matches!(c, Child::Run(_))) {
        return Err(Error::Layout("text directly under the root".into()));
    }
    Ok(json!({
        "root": {
            "type": "root",
            "version": 1,
            "format": "",
            "indent": 0,
            "direction": null,
            "children": children.iter().map(to_json).collect::<Vec<_>>(),
        }
    }))
}

/// Children of a Lexical JSON root as a tree.
pub fn json_root_children(v: &Value) -> Vec<Child> {
    v.get("root")
        .and_then(|r| r.get("children"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(from_json)
        .collect()
}
