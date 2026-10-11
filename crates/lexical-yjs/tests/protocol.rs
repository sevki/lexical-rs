//! The y-protocols sync handshake between two Rust replicas (no JavaScript needed).

use lexical_core::{Command, Editor, EditorState};
use lexical_yjs::YjsDoc;

fn typed(text: &str) -> EditorState {
    let mut e = Editor::new();
    e.dispatch(Command::InsertText(text.into()));
    e.state().clone()
}

#[test]
fn handshake_brings_a_new_peer_up_to_date() {
    let a = YjsDoc::new();
    a.set_state(&typed("hello")).unwrap();
    let b = YjsDoc::new();

    // b announces itself; a answers with what b is missing.
    let reply = a.handle_message(&b.sync_step1()).unwrap().expect("step 2");
    assert!(b.handle_message(&reply).unwrap().is_none());
    assert_eq!(b.state().unwrap().to_plain_text(), "hello");
}

#[test]
fn live_updates_flow_as_messages() {
    let a = YjsDoc::new();
    let b = YjsDoc::new();
    let update = a.set_state(&typed("typed on a")).unwrap();
    b.handle_message(&YjsDoc::update_message(update)).unwrap();
    assert_eq!(b.state().unwrap().to_plain_text(), "typed on a");
}
