#![cfg(feature = "jetstream")]

use jetstream_wireformat::WireFormat;
use lexical_core::*;

fn rich_doc() -> Editor {
    let mut e = Editor::new();
    e.dispatch(Command::Paste("title\nsome bold text\nitem one\nitem two".into()));
    let sel = |e: &mut Editor, a: usize, b: usize| {
        let s = e.state();
        let l = Layout::build(s);
        let (p, q) = (l.point_at(s, a), l.point_at(s, b));
        e.set_selection(p, q);
    };
    sel(&mut e, 0, 0);
    e.dispatch(Command::SetBlockType(BlockType::Heading(HeadingTag::H2)));
    sel(&mut e, 11, 15);
    e.dispatch(Command::FormatText(TextFormat::BOLD));
    e.dispatch(Command::ToggleLink(Some("https://jetstream.rs".into())));
    sel(&mut e, 21, 40);
    e.dispatch(Command::ToggleList(ListType::Check));
    e.dispatch(Command::FormatAlign(Align::Center));
    e
}

#[test]
fn wire_round_trip_matches_json() {
    let e = rich_doc();
    let bytes = e.state().to_wire_bytes().unwrap();
    assert_eq!(bytes.len() as u32, e.state().to_wire().byte_size());
    let back = EditorState::from_wire_bytes(&bytes).unwrap();
    assert_eq!(back.to_json(), e.state().to_json());
    assert_eq!(Layout::build(&back).text, Layout::build(e.state()).text);
}

#[test]
fn editor_state_is_a_wire_format() {
    let e = rich_doc();
    let mut buf = vec![];
    e.state().encode(&mut buf).unwrap();
    assert_eq!(buf.len() as u32, WireFormat::byte_size(e.state()));
    let back = <EditorState as WireFormat>::decode(&mut buf.as_slice()).unwrap();
    assert_eq!(back.to_json(), e.state().to_json());
}

#[test]
fn text_larger_than_u16_string_limit_round_trips() {
    let big = "héllo wörld ".repeat(20_000); // ~260 KB, multi-byte chars
    let mut e = Editor::new();
    e.dispatch(Command::Paste(big.clone()));
    let back = EditorState::from_wire_bytes(&e.state().to_wire_bytes().unwrap()).unwrap();
    assert_eq!(back.to_plain_text(), big);
}

#[test]
fn rejects_garbage_and_bad_versions() {
    assert!(EditorState::from_wire_bytes(&[]).is_err());
    assert!(EditorState::from_wire_bytes(&[0xff; 16]).is_err());
    let mut doc = EditorState::new().to_wire();
    doc.version = 99;
    assert!(EditorState::from_wire(&doc).is_err());
}
