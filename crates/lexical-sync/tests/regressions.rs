//! Behaviours found by review: edits at several places at once keep the text between them,
//! and numbered lists keep their first number across peers.

use lexical_core::{Command, Editor, EditorState, Layout, ListType, NodeData};
use lexical_sync::{flatten, Collab, Replica};

fn place(r: &mut Replica, offset: usize) {
    let (a, f) = {
        let s = r.editor.state();
        let l = Layout::build(s);
        (l.point_at(s, offset), l.point_at(s, offset))
    };
    r.editor.set_selection(a, f);
}

fn exchange(a: &mut Replica, b: &mut Replica) {
    for m in a.drain_updates() {
        b.receive(&m).unwrap();
    }
    for m in b.drain_updates() {
        a.receive(&m).unwrap();
    }
}

#[test]
fn one_update_touching_two_places_does_not_rewrite_the_text_between() {
    let mut a = Replica::new(1).unwrap();
    let mut b = Replica::new(2).unwrap();
    a.dispatch(Command::InsertText("one two three".into()));
    exchange(&mut a, &mut b);

    // A (say a plugin) changes both ends in a single update ...
    a.editor
        .update(|s| {
            let block = s.line_blocks()[0];
            let text = s.children(block)[0];
            s.set_text(text, "ONE two THREE");
            Ok(())
        })
        .unwrap();
    // ... while B types inside the untouched middle.
    place(&mut b, 5);
    b.dispatch(Command::InsertText("X".into()));
    exchange(&mut a, &mut b);

    assert_eq!(a.text(), "ONE tXwo THREE", "B's edit stays inside the original word");
    assert_eq!(b.text(), a.text());
    assert_eq!(flatten(a.editor.state()), flatten(b.editor.state()));
}

fn numbered_from(start: u64) -> EditorState {
    let mut editor = Editor::new();
    editor.dispatch(Command::InsertText("five".into()));
    editor.dispatch(Command::ToggleList(ListType::Number));
    let mut json = editor.state().to_json();
    fn set(v: &mut serde_json::Value, start: u64) {
        if let Some(o) = v.as_object_mut() {
            if o.get("type").and_then(|t| t.as_str()) == Some("list") {
                o.insert("start".into(), start.into());
            }
        }
        match v {
            serde_json::Value::Object(o) => o.values_mut().for_each(|c| set(c, start)),
            serde_json::Value::Array(a) => a.iter_mut().for_each(|c| set(c, start)),
            _ => {}
        }
    }
    set(&mut json, start);
    EditorState::from_json(&json).unwrap()
}

fn list_start(state: &EditorState) -> Option<u32> {
    let item = state.line_blocks()[0];
    let list = state.parent(item)?;
    match &state.node(list).data {
        NodeData::List { start, .. } => Some(*start),
        _ => None,
    }
}

#[test]
fn a_numbered_list_keeps_its_start_for_everyone() {
    let mut editor = Editor::new();
    editor.set_state(numbered_from(5));
    assert_eq!(list_start(editor.state()), Some(5), "fixture");
    let collab = Collab::attach(&mut editor, 1).unwrap();

    let joined = Replica::join(2, &collab.snapshot().unwrap()).unwrap();
    assert_eq!(list_start(joined.editor.state()), Some(5), "a late joiner sees 5, not 1");

    // the originating editor rebuilds its document on the next remote update
    let mut origin = Replica { editor, collab };
    let mut other = joined;
    other.dispatch(Command::InsertText("!".into()));
    exchange(&mut origin, &mut other);
    assert_eq!(list_start(origin.editor.state()), Some(5));
    assert_eq!(flatten(origin.editor.state()), flatten(other.editor.state()));
}
