//! What a collaborator experiences beyond merging: undo that only touches their own
//! edits, a selection that stays on the same text, remote carets, and read-only editors.

use lexical_core::{Command, Editor, Layout, Point, TextFormat};
use lexical_sync::flat::flat_offset;
use lexical_sync::{flatten, Collab, Replica, SyncOptions};

fn place(r: &mut Replica, offset: usize) {
    select(r, offset, offset);
}

fn select(r: &mut Replica, from: usize, to: usize) {
    let (a, f) = {
        let s = r.editor.state();
        let l = Layout::build(s);
        (l.point_at(s, from), l.point_at(s, to))
    };
    r.editor.set_selection(a, f);
}

fn text(r: &Replica) -> String {
    r.editor.state().to_plain_text()
}

fn exchange(a: &mut Replica, b: &mut Replica) {
    for m in a.drain_updates() {
        b.receive(&m).unwrap();
    }
    for m in b.drain_updates() {
        a.receive(&m).unwrap();
    }
}

/// Every local commit is its own undo step, so tests do not depend on timing.
fn pair() -> (Replica, Replica) {
    let opts = SyncOptions { undo_merge_ms: 0 };
    let mut a = Replica::with_options(1, opts).unwrap();
    let mut b = Replica::with_options(2, opts).unwrap();
    a.dispatch(Command::InsertText("base".into()));
    exchange(&mut a, &mut b);
    (a, b)
}

#[test]
fn undo_only_reverts_my_own_edits() {
    let (mut a, mut b) = pair();
    place(&mut a, 0);
    a.dispatch(Command::InsertText("A".into()));
    place(&mut b, 4);
    b.dispatch(Command::InsertText("B".into()));
    exchange(&mut a, &mut b);
    assert_eq!((text(&a).as_str(), text(&b).as_str()), ("AbaseB", "AbaseB"));

    a.dispatch(Command::Undo);
    assert_eq!(text(&a), "baseB", "A's undo removed A's text and kept B's");
    exchange(&mut a, &mut b);
    assert_eq!(text(&b), "baseB");

    // B can undo their own edit, which does not bring back A's
    b.dispatch(Command::Undo);
    exchange(&mut a, &mut b);
    assert_eq!((text(&a).as_str(), text(&b).as_str()), ("base", "base"));

    // and redo restores it for everyone
    b.dispatch(Command::Redo);
    exchange(&mut a, &mut b);
    assert_eq!((text(&a).as_str(), text(&b).as_str()), ("baseB", "baseB"));
    a.editor.state().check_invariants().unwrap();
    b.editor.state().check_invariants().unwrap();
}

#[test]
fn undo_availability_is_per_peer_and_the_shared_start_is_not_undoable() {
    let opts = SyncOptions { undo_merge_ms: 0 };
    let mut a = Replica::with_options(1, opts).unwrap();
    let mut b = Replica::with_options(2, opts).unwrap();
    assert!(!a.editor.can_undo(), "the shared initial paragraph is not an edit");
    a.dispatch(Command::InsertText("hi".into()));
    assert!(a.editor.can_undo());
    exchange(&mut a, &mut b);
    assert!(!b.editor.can_undo(), "B did not make that edit");
    assert!(!a.editor.history().can_undo(), "the built-in snapshot history is not used");
    a.dispatch(Command::Undo);
    assert!(!a.editor.can_undo() && a.editor.can_redo());
}

#[test]
fn undo_is_ignored_when_the_editor_is_read_only() {
    let (mut a, _b) = pair();
    a.dispatch(Command::InsertText("x".into()));
    a.editor.set_editable(false);
    assert!(!a.dispatch(Command::Undo));
    assert!(!a.dispatch(Command::InsertText("y".into())));
    assert_eq!(text(&a), "basex", "neither undo nor typing changed a read-only editor");
}

#[test]
fn the_caret_stays_on_the_same_text_when_a_peer_edits_before_it() {
    let (mut a, mut b) = pair();
    place(&mut a, 4); // end of "base"
    place(&mut b, 0);
    b.dispatch(Command::InsertText("XX".into()));
    exchange(&mut a, &mut b);
    assert_eq!(text(&a), "XXbase");
    a.dispatch(Command::InsertText("!".into()));
    assert_eq!(text(&a), "XXbase!", "A's caret followed its character instead of staying at offset 4");
}

#[test]
fn a_selection_follows_remote_edits_around_it() {
    let (mut a, mut b) = pair();
    select(&mut a, 1, 3); // "as"
    place(&mut b, 0);
    b.dispatch(Command::InsertText("Q".into()));
    exchange(&mut a, &mut b);
    a.dispatch(Command::FormatText(TextFormat::BOLD));
    exchange(&mut a, &mut b);
    let l = Layout::build(b.editor.state());
    assert_eq!(l.text, "Qbase");
    let bold: Vec<_> = l.runs.iter().filter(|r| r.format.contains(TextFormat::BOLD)).map(|r| (r.start, r.end)).collect();
    assert_eq!(bold, [(2, 4)], "still the same two characters");
}

#[test]
fn a_remote_edit_inside_a_selection_keeps_it_selected() {
    let (mut a, mut b) = pair();
    select(&mut a, 0, 4);
    place(&mut b, 2);
    b.dispatch(Command::InsertText("-".into()));
    exchange(&mut a, &mut b);
    assert_eq!(text(&a), "ba-se");
    assert_eq!(a.editor.state().selected_text(), "ba-se");
}

#[test]
fn remote_carets_land_on_the_right_characters() {
    let (mut a, mut b) = pair();
    place(&mut a, 2);
    let msg = a.collab.local_presence(&a.editor, "Ada", "#3584e4").expect("presence");
    let caret = b.collab.resolve_presence(&b.editor, &msg).unwrap();
    assert_eq!((caret.name.as_str(), caret.color.as_str(), caret.peer), ("Ada", "#3584e4", 1));
    assert_eq!(flat_offset(b.editor.state(), &caret.focus), 2);
    assert_eq!(caret.anchor, caret.focus);

    // B edits before A's caret; the same message now resolves further along
    place(&mut b, 0);
    b.dispatch(Command::InsertText("ZZ".into()));
    let caret = b.collab.resolve_presence(&b.editor, &msg).unwrap();
    assert_eq!(flat_offset(b.editor.state(), &caret.focus), 4);
}

#[test]
fn malformed_presence_is_an_error_not_a_panic() {
    let (a, _b) = pair();
    for junk in [&b""[..], b"{}", br#"{"peer":1,"anchor":"x","focus":[1]}"#, br#"{"peer":1,"anchor":[300],"focus":[]}"#] {
        assert!(a.collab.resolve_presence(&a.editor, junk).is_err(), "{junk:?}");
    }
}

#[test]
fn attaching_to_an_editor_with_content_makes_it_the_shared_document() {
    let mut editor = Editor::new();
    editor.dispatch(Command::Paste("already here\nsecond line".into()));
    let collab = Collab::attach(&mut editor, 1).unwrap();
    let snapshot = collab.snapshot().unwrap();
    let joined = Replica::join(2, &snapshot).unwrap();
    assert_eq!(flatten(joined.editor.state()), flatten(editor.state()));
    assert_eq!(joined.editor.state().to_plain_text(), "already here\n\nsecond line");
    assert!(collab.take_errors().is_empty());
}

#[test]
fn views_see_remote_changes_as_remote_updates() {
    use lexical_core::Tag;
    use std::cell::RefCell;
    use std::rc::Rc;
    let (mut a, mut b) = pair();
    let seen = Rc::new(RefCell::new(vec![]));
    let s = seen.clone();
    b.editor.register_update_listener(move |ev| {
        s.borrow_mut().push((ev.has_tag(Tag::Remote), ev.content_changed()));
    });
    a.dispatch(Command::InsertText("!".into()));
    exchange(&mut a, &mut b);
    assert_eq!(*seen.borrow(), [(true, true)]);
    b.dispatch(Command::InsertText("?".into()));
    assert_eq!(seen.borrow().last(), Some(&(false, true)), "a local edit is not tagged remote");
    let _: Point = b.editor.state().selection.as_ref().unwrap().anchor;
}
