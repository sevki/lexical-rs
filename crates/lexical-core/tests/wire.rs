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
fn the_document_types_are_the_wire_types() {
    // No separate wire model: the types the editor edits are the types that are encoded.
    let e = rich_doc();
    let mut buf = vec![];
    WireFormat::encode(e.state(), &mut buf).unwrap();
    assert_eq!(buf.len() as u32, WireFormat::byte_size(e.state()));
    let mut back = <EditorState as WireFormat>::decode(&mut buf.as_slice()).unwrap();
    back.check_wire().unwrap();
    assert_eq!(back.to_json(), e.state().to_json());
    assert_eq!(back.selection, e.state().selection, "the selection travels with the document");
}

#[test]
fn bytes_round_trip_and_are_canonical() {
    let e = rich_doc();
    let bytes = e.state().to_wire_bytes().unwrap();
    assert_eq!(bytes[0], WIRE_VERSION_FOR_TESTS);
    let back = EditorState::from_wire_bytes(&bytes).unwrap();
    assert_eq!(back.to_json(), e.state().to_json());
    assert_eq!(Layout::build(&back).text, Layout::build(e.state()).text);
    // equal documents give equal bytes, whatever order the arena happens to iterate in
    assert_eq!(back.to_wire_bytes().unwrap(), bytes);
    assert_eq!(e.state().clone().to_wire_bytes().unwrap(), bytes);
}

const WIRE_VERSION_FOR_TESTS: u8 = lexical_core::wire::WIRE_VERSION;

#[test]
fn text_larger_than_a_jetstream_string_round_trips() {
    let big = "héllo wörld ".repeat(20_000); // ~260 KB, multi-byte chars
    let mut e = Editor::new();
    e.dispatch(Command::Paste(big.clone()));
    let back = EditorState::from_wire_bytes(&e.state().to_wire_bytes().unwrap()).unwrap();
    assert_eq!(back.to_plain_text(), big);
}

#[test]
fn more_nodes_and_children_than_a_jetstream_vector_round_trip() {
    let mut s = EditorState::empty();
    for i in 0..70_000u32 {
        let p = s.create_node(NodeData::Paragraph);
        s.append_child(ROOT_KEY, p);
        let t = s.create_node(NodeData::text(&format!("line {i}"), TextFormat::empty()));
        s.append_child(p, t);
    }
    s.clear_dirty();
    s.check_invariants().unwrap();
    let back = EditorState::from_wire_bytes(&s.to_wire_bytes().unwrap()).unwrap();
    assert_eq!(back.root_children().len(), 70_000);
    assert_eq!(back.to_plain_text(), s.to_plain_text());
}

#[test]
fn rejects_malformed_input() {
    let good = rich_doc().state().to_wire_bytes().unwrap();
    assert!(EditorState::from_wire_bytes(&[]).is_err());
    assert!(EditorState::from_wire_bytes(&[0xff; 16]).is_err());

    let mut wrong_version = good.clone();
    wrong_version[0] = 99;
    assert!(EditorState::from_wire_bytes(&wrong_version).is_err());

    let mut trailing = good.clone();
    trailing.push(0);
    assert!(EditorState::from_wire_bytes(&trailing).is_err());

    // every truncation fails cleanly
    for n in 0..good.len() {
        assert!(EditorState::from_wire_bytes(&good[..n]).is_err(), "prefix of {n} bytes");
    }
}

#[test]
fn a_hostile_length_does_not_decide_the_allocation() {
    // version, then a node count of u32::MAX with no nodes behind it
    let bytes = [lexical_core::wire::WIRE_VERSION, 0xff, 0xff, 0xff, 0xff];
    assert!(EditorState::from_wire_bytes(&bytes).is_err());
}

#[test]
fn a_decoded_arena_that_is_not_a_valid_tree_is_refused() {
    // an orphan: reachable from nowhere
    let mut orphan = EditorState::new();
    orphan.create_node(NodeData::Paragraph);
    assert!(EditorState::from_wire_bytes(&orphan.to_wire_bytes().unwrap()).is_err());

    // a text node with a child
    let mut leaf = EditorState::new();
    let p = leaf.root_children()[0];
    let t = leaf.create_node(NodeData::text("x", TextFormat::empty()));
    leaf.append_child(p, t);
    let inner = leaf.create_node(NodeData::text("y", TextFormat::BOLD));
    leaf.append_child(t, inner);
    assert!(EditorState::from_wire_bytes(&leaf.to_wire_bytes().unwrap()).is_err());

    // the raw `WireFormat::decode` accepts these bytes; `check_wire` is what refuses them
    let mut raw = vec![];
    WireFormat::encode(&leaf, &mut raw).unwrap();
    let mut decoded = <EditorState as WireFormat>::decode(&mut raw.as_slice()).unwrap();
    assert!(decoded.check_wire().is_err());
}
