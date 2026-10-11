//! Write a desired tree into a Yjs document as a minimal set of edits.
//!
//! Children are compared by position: an unchanged prefix and suffix are left alone, and
//! in the middle equal-length runs are updated in place where the kinds match, otherwise
//! replaced. Text inside a node is diffed by common prefix and suffix, so typing becomes a
//! small insert or delete that merges with concurrent edits.

use crate::tree::{as_f64, json_root_children, utf16_len, Child, Elem, Leaf, Props, Run};
use crate::Result;
use serde_json::Value;
use yrs::types::xml::Xml;
use yrs::{Any, Map, MapPrelim, Text, TextRef, TransactionMut, XmlTextPrelim};

pub fn sync(txn: &mut TransactionMut, root: &TextRef, desired: &Value) -> Result<()> {
    let old = crate::tree::read_children(txn, root);
    let new = json_root_children(desired);
    children(txn, root, &old, &new);
    Ok(())
}

fn any_eq(old: Option<&Any>, new: &Any) -> bool {
    match (old, new) {
        (None | Some(Any::Undefined | Any::Null), Any::Undefined) => true,
        (Some(o), n) => match (as_f64(o), as_f64(n)) {
            (Some(a), Some(b)) => a == b,
            _ => o == n,
        },
        (None, _) => false,
    }
}

/// `new` is satisfied by `old`: every property we want is already there. Properties only
/// the old side has (Lexical writes some we do not model) are left alone.
fn props_eq(old: &Props, new: &Props, skip: &[&str]) -> bool {
    new.iter().all(|(k, v)| skip.contains(&k.as_str()) || any_eq(old.get(k), v))
}

/// Tab and code-highlight nodes are text nodes here; do not fight over their type.
fn run_skips(old: &Run) -> &'static [&'static str] {
    match old.props.get("__type") {
        Some(Any::String(t)) if &**t == "text" => &[],
        _ => &["__type", "__detail"],
    }
}

fn same(old: &Child, new: &Child) -> bool {
    match (old, new) {
        (Child::Elem(o), Child::Elem(n)) => {
            props_eq(&o.props, &n.props, &[])
                && o.children.len() == n.children.len()
                && o.children.iter().zip(&n.children).all(|(a, b)| same(a, b))
        }
        (Child::Run(o), Child::Run(n)) => o.text == n.text && props_eq(&o.props, &n.props, run_skips(o)),
        (Child::Leaf(o), Child::Leaf(n)) => props_eq(&o.props, &n.props, &[]),
        _ => false,
    }
}

/// Can `old` be edited into `new` in place?
fn compatible(old: &Child, new: &Child) -> bool {
    match (old, new) {
        (Child::Elem(_), Child::Elem(_)) => old.type_name() == new.type_name(),
        (Child::Run(_), Child::Run(_)) => true,
        _ => false,
    }
}

fn children<X: Text>(txn: &mut TransactionMut, parent: &X, old: &[Child], new: &[Child]) {
    let prefix = old.iter().zip(new).take_while(|(a, b)| same(a, b)).count();
    let room = old.len().min(new.len()) - prefix;
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(room)
        .take_while(|(a, b)| same(a, b))
        .count();
    let mut pos: u32 = old[..prefix].iter().map(Child::len).sum();
    let mid_old = &old[prefix..old.len() - suffix];
    let mid_new = &new[prefix..new.len() - suffix];

    if mid_old.len() == mid_new.len() {
        for (o, n) in mid_old.iter().zip(mid_new) {
            if compatible(o, n) {
                update(txn, parent, pos, o, n);
            } else {
                parent.remove_range(txn, pos, o.len());
                insert(txn, parent, pos, n);
            }
            pos += n.len();
        }
        return;
    }
    let remove: u32 = mid_old.iter().map(Child::len).sum();
    if remove > 0 {
        parent.remove_range(txn, pos, remove);
    }
    for n in mid_new {
        pos += insert(txn, parent, pos, n);
    }
}

fn set_changed(txn: &mut TransactionMut, old: &Props, new: &Props, skip: &[&str], mut set: impl FnMut(&mut TransactionMut, &str, Any)) {
    for (k, v) in new {
        if skip.contains(&k.as_str()) || any_eq(old.get(k), v) {
            continue;
        }
        set(txn, k, v.clone());
    }
}

/// Edit the node at `pos` in `parent` from `old` to `new` (same kind).
fn update<X: Text>(txn: &mut TransactionMut, parent: &X, pos: u32, old: &Child, new: &Child) {
    match (old, new) {
        (Child::Elem(o), Child::Elem(n)) => {
            let Some(h) = &o.handle else { return };
            set_changed(txn, &o.props, &n.props, &[], |txn, k, v| {
                h.insert_attribute(txn, k.to_string(), v);
            });
            children(txn, h, &o.children, &n.children);
        }
        (Child::Run(o), Child::Run(n)) => {
            if let Some(h) = &o.handle {
                set_changed(txn, &o.props, &n.props, run_skips(o), |txn, k, v| {
                    h.insert(txn, k.to_string(), v);
                });
            }
            text(txn, parent, pos + 1, &o.text, &n.text);
        }
        _ => {}
    }
}

/// Replace `old` by `new` at `at` touching only the characters in between.
fn text<X: Text>(txn: &mut TransactionMut, parent: &X, at: u32, old: &str, new: &str) {
    if old == new {
        return;
    }
    let o: Vec<char> = old.chars().collect();
    let n: Vec<char> = new.chars().collect();
    let p = o.iter().zip(&n).take_while(|(a, b)| a == b).count();
    let s = o[p..].iter().rev().zip(n[p..].iter().rev()).take_while(|(a, b)| a == b).count();
    let units = |cs: &[char]| cs.iter().map(|c| c.len_utf16() as u32).sum::<u32>();
    let start = at + units(&o[..p]);
    let del = units(&o[p..o.len() - s]);
    if del > 0 {
        parent.remove_range(txn, start, del);
    }
    let ins: String = n[p..n.len() - s].iter().collect();
    if !ins.is_empty() {
        parent.insert(txn, start, &ins);
    }
}

fn map_prelim(props: &Props) -> MapPrelim {
    props.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// Insert `c` at `pos`; returns its length in Yjs units.
fn insert<X: Text>(txn: &mut TransactionMut, parent: &X, pos: u32, c: &Child) -> u32 {
    match c {
        Child::Elem(Elem { props, children, .. }) => {
            let h = parent.insert_embed(txn, pos, XmlTextPrelim::new(""));
            for (k, v) in props {
                h.insert_attribute(txn, k.clone(), v.clone());
            }
            let mut at = 0;
            for ch in children {
                at += insert(txn, &h, at, ch);
            }
        }
        Child::Run(Run { props, text, .. }) => {
            parent.insert_embed(txn, pos, map_prelim(props));
            if !text.is_empty() {
                parent.insert(txn, pos + 1, text);
            }
            return 1 + utf16_len(text);
        }
        Child::Leaf(Leaf { props, .. }) => {
            parent.insert_embed(txn, pos, map_prelim(props));
        }
    }
    1
}
