use lexical_core::*;

fn editor_with(text: &str) -> Editor {
    let mut e = Editor::new();
    e.dispatch(Command::Paste(text.into()));
    e
}

fn lines(e: &Editor) -> Vec<String> {
    let s = e.state();
    s.line_blocks()
        .iter()
        .map(|&b| s.block_content(b).text)
        .collect()
}

fn select(e: &mut Editor, from: usize, to: usize) {
    let s = e.state();
    let l = Layout::build(s);
    let (a, f) = (l.point_at(s, from), l.point_at(s, to));
    e.set_selection(a, f);
}

fn caret(e: &Editor) -> usize {
    let s = e.state();
    Layout::build(s).offset_of(s, &s.selection.as_ref().unwrap().focus)
}

#[test]
fn typing_and_paragraphs() {
    let mut e = Editor::new();
    e.dispatch(Command::InsertText("Hello".into()));
    e.dispatch(Command::InsertParagraph);
    e.dispatch(Command::InsertText("World".into()));
    assert_eq!(lines(&e), ["Hello", "World"]);
    assert_eq!(e.state().to_plain_text(), "Hello\n\nWorld");
}

#[test]
fn backspace_merges_blocks_and_chars() {
    let mut e = editor_with("ab\ncd");
    select(&mut e, 3, 3);
    e.dispatch(Command::DeleteCharacter { backward: true });
    assert_eq!(lines(&e), ["abcd"]);
    assert_eq!(caret(&e), 2);
    e.dispatch(Command::DeleteCharacter { backward: true });
    assert_eq!(lines(&e), ["acd"]);
    e.dispatch(Command::DeleteCharacter { backward: false });
    assert_eq!(lines(&e), ["ad"]);
}

#[test]
fn delete_range_across_blocks() {
    let mut e = editor_with("one\ntwo\nthree");
    select(&mut e, 2, 10);
    e.dispatch(Command::DeleteCharacter { backward: true });
    assert_eq!(lines(&e), ["onree"]);
}

#[test]
fn delete_grapheme_clusters() {
    let mut e = editor_with("a👨‍👩‍👧b");
    select(&mut e, 5, 5); // before 'b'? family emoji is 5 chars
    let end = Layout::build(e.state()).char_len();
    select(&mut e, end - 1, end - 1);
    e.dispatch(Command::DeleteCharacter { backward: true });
    assert_eq!(lines(&e), ["ab"]);
}

#[test]
fn word_delete() {
    let mut e = editor_with("hello brave world");
    e.dispatch(Command::DeleteWord { backward: true });
    assert_eq!(lines(&e), ["hello brave "]);
}

#[test]
fn bold_toggle_range_and_typing() {
    let mut e = editor_with("hello world");
    select(&mut e, 0, 5);
    e.dispatch(Command::FormatText(TextFormat::BOLD));
    let l = Layout::build(e.state());
    assert!(
        l.runs
            .iter()
            .any(|r| r.start == 0 && r.end == 5 && r.format == TextFormat::BOLD)
    );
    assert!(l.runs.iter().any(|r| r.start == 5 && r.format.is_empty()));
    assert_eq!(e.state().selection_format(), TextFormat::BOLD);
    // toggling again removes it, and runs merge back into one node
    e.dispatch(Command::FormatText(TextFormat::BOLD));
    assert_eq!(Layout::build(e.state()).runs.len(), 1);

    // pending format with a collapsed caret
    select(&mut e, 11, 11);
    e.dispatch(Command::FormatText(TextFormat::ITALIC));
    e.dispatch(Command::InsertText("!".into()));
    let l = Layout::build(e.state());
    assert_eq!(l.runs.last().unwrap().format, TextFormat::ITALIC);
    assert_eq!(l.text, "hello world!");
}

#[test]
fn block_types_and_enter_rules() {
    let mut e = editor_with("title");
    e.dispatch(Command::SetBlockType(BlockType::Heading(HeadingTag::H2)));
    assert_eq!(
        Layout::build(e.state()).lines[0].style,
        BlockStyle::Heading(HeadingTag::H2)
    );
    e.dispatch(Command::InsertParagraph); // at end: new paragraph
    assert_eq!(
        Layout::build(e.state()).lines[1].style,
        BlockStyle::Paragraph
    );
    // backspace at start of heading turns it into a paragraph
    select(&mut e, 0, 0);
    e.dispatch(Command::DeleteCharacter { backward: true });
    assert_eq!(
        Layout::build(e.state()).lines[0].style,
        BlockStyle::Paragraph
    );
}

#[test]
fn lists_toggle_nest_and_exit() {
    let mut e = editor_with("a\nb\nc");
    e.dispatch(Command::SelectAll);
    e.dispatch(Command::ToggleList(ListType::Number));
    let l = Layout::build(e.state());
    assert_eq!(l.text, "1. a\n2. b\n3. c");
    // single list after merge
    assert_eq!(e.state().root_children().len(), 1);

    select(&mut e, 7, 7); // inside "b"
    e.dispatch(Command::Indent);
    let l = Layout::build(e.state());
    assert!(matches!(
        l.lines[1].style,
        BlockStyle::ListItem { depth: 1, .. }
    ));
    e.dispatch(Command::Outdent);
    let l = Layout::build(e.state());
    assert!(matches!(
        l.lines[1].style,
        BlockStyle::ListItem { depth: 0, .. }
    ));

    // Enter on an empty item leaves the list
    select(&mut e, 14, 14);
    e.dispatch(Command::InsertParagraph);
    e.dispatch(Command::InsertParagraph);
    let l = Layout::build(e.state());
    assert_eq!(l.lines.last().unwrap().style, BlockStyle::Paragraph);
    assert_eq!(lines(&e).len(), 4);

    // a mixed selection (items + the plain paragraph) extends the list over everything
    e.dispatch(Command::SelectAll);
    e.dispatch(Command::ToggleList(ListType::Number));
    let l = Layout::build(e.state());
    assert!(
        l.lines
            .iter()
            .all(|l| matches!(l.style, BlockStyle::ListItem { .. }))
    );
    assert_eq!(e.state().root_children().len(), 1, "one merged list");

    // toggling the same list type again, now that every block is an item, removes it
    e.dispatch(Command::ToggleList(ListType::Number));
    assert!(
        Layout::build(e.state())
            .lines
            .iter()
            .all(|l| l.style == BlockStyle::Paragraph)
    );
    e.state().check_invariants().unwrap();
}

#[test]
fn check_list_toggle() {
    let mut e = editor_with("task");
    e.dispatch(Command::ToggleList(ListType::Check));
    assert_eq!(Layout::build(e.state()).text, "☐ task");
    e.dispatch(Command::ToggleCheck);
    assert_eq!(Layout::build(e.state()).text, "☑ task");
}

#[test]
fn links() {
    let mut e = editor_with("see docs here");
    select(&mut e, 4, 8);
    e.dispatch(Command::ToggleLink(Some("https://example.com".into())));
    let l = Layout::build(e.state());
    let r = l.runs.iter().find(|r| r.link.is_some()).unwrap();
    assert_eq!((r.start, r.end), (4, 8));
    assert_eq!(
        e.state().link_at_selection().as_deref(),
        Some("https://example.com")
    );
    e.dispatch(Command::ToggleLink(None));
    assert!(
        Layout::build(e.state())
            .runs
            .iter()
            .all(|r| r.link.is_none())
    );
}

#[test]
fn relinking_part_of_a_link_leaves_the_rest_alone() {
    let mut e = editor_with("abcd");
    select(&mut e, 0, 4);
    e.dispatch(Command::ToggleLink(Some("old".into())));
    select(&mut e, 1, 3);
    e.dispatch(Command::ToggleLink(Some("new".into())));
    let l = Layout::build(e.state());
    let url_at = |off: usize| {
        l.runs
            .iter()
            .find(|r| r.start <= off && off < r.end)
            .unwrap()
            .link
            .clone()
    };
    assert_eq!(url_at(0).as_deref(), Some("old"));
    assert_eq!(url_at(1).as_deref(), Some("new"));
    assert_eq!(url_at(2).as_deref(), Some("new"));
    assert_eq!(url_at(3).as_deref(), Some("old"));
    e.state().check_invariants().unwrap();

    // removing the link from the middle keeps the ends linked
    select(&mut e, 1, 3);
    e.dispatch(Command::ToggleLink(None));
    let l = Layout::build(e.state());
    let url_at = |off: usize| {
        l.runs
            .iter()
            .find(|r| r.start <= off && off < r.end)
            .unwrap()
            .link
            .clone()
    };
    assert_eq!(url_at(0).as_deref(), Some("old"));
    assert_eq!(url_at(1), None);
    assert_eq!(url_at(2), None);
    assert_eq!(url_at(3).as_deref(), Some("old"));
    e.state().check_invariants().unwrap();
}

#[test]
fn changing_one_items_list_type_does_not_convert_its_siblings() {
    let mut e = editor_with("a\nb\nc");
    e.dispatch(Command::SelectAll);
    e.dispatch(Command::ToggleList(ListType::Number));
    select(&mut e, 7, 7); // inside "b"
    e.dispatch(Command::ToggleList(ListType::Bullet));
    assert_eq!(Layout::build(e.state()).text, "1. a\n• b\n1. c");
    e.state().check_invariants().unwrap();
    // selecting everything and toggling bullets makes one merged bullet list
    e.dispatch(Command::SelectAll);
    e.dispatch(Command::ToggleList(ListType::Bullet));
    assert_eq!(Layout::build(e.state()).text, "• a\n• b\n• c");
    assert_eq!(e.state().root_children().len(), 1);
}

fn first_item_depth(e: &Editor) -> u32 {
    match Layout::build(e.state()).lines[0].style {
        BlockStyle::ListItem { depth, .. } => depth,
        _ => unreachable!(),
    }
}

#[test]
fn list_nesting_is_unlimited_by_default() {
    let mut e = editor_with("deep");
    e.dispatch(Command::ToggleList(ListType::Bullet));
    for _ in 0..20 {
        e.dispatch(Command::Indent);
        e.state().check_invariants().unwrap();
    }
    assert_eq!(first_item_depth(&e), 20);
}

#[test]
fn list_nesting_and_indent_can_be_capped_by_the_host() {
    let mut e = editor_with("deep");
    e.set_limits(Limits {
        max_list_depth: Some(3),
        max_indent: Some(2),
    });
    e.dispatch(Command::ToggleList(ListType::Bullet));
    for _ in 0..10 {
        e.dispatch(Command::Indent);
    }
    assert_eq!(first_item_depth(&e), 3);

    // plain blocks honour max_indent, and the cap survives loading a document
    let mut p = editor_with("para");
    p.set_limits(Limits {
        max_list_depth: None,
        max_indent: Some(2),
    });
    for _ in 0..10 {
        p.dispatch(Command::Indent);
    }
    let json = p.state().to_json();
    assert_eq!(json["root"]["children"][0]["indent"], 2);
    p.set_state(EditorState::from_json(&json).unwrap());
    p.dispatch(Command::Indent);
    assert_eq!(p.state().to_json()["root"]["children"][0]["indent"], 2);
}

#[test]
fn read_only_editor_ignores_history_and_edits() {
    let mut e = editor_with("keep");
    e.dispatch(Command::InsertText("!".into()));
    assert!(e.can_undo());
    e.set_editable(false);
    assert!(
        !e.dispatch(Command::Undo),
        "undo must not mutate a read-only editor"
    );
    assert!(!e.dispatch(Command::Redo));
    assert!(!e.dispatch(Command::InsertText("x".into())));
    assert_eq!(lines(&e), ["keep!"]);
    // selection-only commands still work
    assert!(e.dispatch(Command::SelectAll));
    e.set_editable(true);
    assert!(e.dispatch(Command::Undo));
    assert_eq!(lines(&e), ["keep"]);
}

#[test]
fn enter_inside_link_splits_it() {
    let mut e = editor_with("abcd");
    select(&mut e, 0, 4);
    e.dispatch(Command::ToggleLink(Some("u".into())));
    select(&mut e, 2, 2);
    e.dispatch(Command::InsertParagraph);
    let l = Layout::build(e.state());
    assert_eq!(l.text, "ab\ncd");
    assert!(l.runs.iter().all(|r| r.link.as_deref() == Some("u")));
}

#[test]
fn code_block_enter_inserts_linebreak_and_double_enter_exits() {
    let mut e = editor_with("x");
    e.dispatch(Command::SetBlockType(BlockType::Code));
    e.dispatch(Command::InsertParagraph);
    e.dispatch(Command::InsertText("y".into()));
    assert_eq!(lines(&e), ["x\ny"]);
    e.dispatch(Command::InsertParagraph);
    e.dispatch(Command::InsertParagraph);
    assert_eq!(lines(&e).len(), 2);
    assert_eq!(
        Layout::build(e.state()).lines[1].style,
        BlockStyle::Paragraph
    );
}

#[test]
fn undo_redo_with_typing_coalescing() {
    let mut e = Editor::new();
    for c in "hey".chars() {
        e.dispatch(Command::InsertText(c.to_string()));
    }
    e.dispatch(Command::InsertParagraph);
    e.dispatch(Command::InsertText("x".into()));
    assert_eq!(lines(&e), ["hey", "x"]);
    e.dispatch(Command::Undo); // "x"
    assert_eq!(lines(&e), ["hey", ""]);
    e.dispatch(Command::Undo); // paragraph
    assert_eq!(lines(&e), ["hey"]);
    e.dispatch(Command::Undo); // all of "hey" as one step
    assert_eq!(lines(&e), [""]);
    assert!(!e.dispatch(Command::Undo));
    e.dispatch(Command::Redo);
    assert_eq!(lines(&e), ["hey"]);
    // new edits clear redo
    e.dispatch(Command::InsertText("!".into()));
    assert!(!e.can_redo());
}

#[test]
fn failed_update_rolls_back() {
    let mut e = editor_with("keep");
    let r = e.update(|s| {
        s.insert_text("zzz")?;
        Err(Error::Invalid("nope".into()))
    });
    assert!(r.is_err());
    assert_eq!(lines(&e), ["keep"]);
}

#[test]
fn listeners_and_transforms() {
    use std::cell::RefCell;
    use std::rc::Rc;
    let seen = Rc::new(RefCell::new(0));
    let s2 = seen.clone();
    let mut e = Editor::new();
    e.register_update_listener(move |ev| {
        if ev.content_changed() {
            *s2.borrow_mut() += 1;
        }
    });
    e.register_node_transform(NodeType::Text, |s, k| {
        let t = s.node(k).text().unwrap().to_string();
        if t.contains("foo") {
            s.set_text(k, &t.replace("foo", "bar"));
        }
        Ok(())
    });
    e.dispatch(Command::InsertText("a foo b".into()));
    assert_eq!(lines(&e), ["a bar b"]);
    assert_eq!(*seen.borrow(), 1);
}

#[test]
fn runaway_transform_errors() {
    let mut e = Editor::new();
    e.register_node_transform(NodeType::Text, |s, k| {
        let t = s.node(k).text().unwrap().to_string();
        s.set_text(k, &format!("{t}x"));
        Ok(())
    });
    let r = e.update(|s| s.insert_text("a"));
    assert_eq!(r, Err(Error::TransformLoop));
}

#[test]
fn markdown_shortcuts() {
    let mut e = Editor::new();
    e.add_plugin(Box::new(MarkdownShortcutsPlugin::default()));
    e.dispatch(Command::InsertText("#".into()));
    e.dispatch(Command::InsertText(" ".into()));
    assert!(matches!(
        Layout::build(e.state()).lines[0].style,
        BlockStyle::Heading(HeadingTag::H1)
    ));
    e.dispatch(Command::InsertText("Hi".into()));
    assert_eq!(lines(&e), ["Hi"]);

    e.dispatch(Command::InsertParagraph); // new paragraph (end of heading)
    e.dispatch(Command::InsertText("- ".into()));
    let l = Layout::build(e.state());
    assert!(matches!(
        l.lines[1].style,
        BlockStyle::ListItem {
            list_type: ListType::Bullet,
            ..
        }
    ));
}

#[test]
fn json_round_trip_and_lexical_shape() {
    let mut e = editor_with("hello");
    select(&mut e, 0, 5);
    e.dispatch(Command::FormatText(TextFormat::BOLD));
    e.dispatch(Command::SetBlockType(BlockType::Heading(HeadingTag::H1)));
    let json = e.state().to_json();
    let p = &json["root"]["children"][0];
    assert_eq!(p["type"], "heading");
    assert_eq!(p["tag"], "h1");
    assert_eq!(p["children"][0]["format"], 1);
    assert_eq!(p["children"][0]["text"], "hello");

    let back = EditorState::from_json(&json).unwrap();
    assert_eq!(back.to_json(), json);
    assert!(EditorState::from_json_str("{}").is_err());
}

#[test]
fn imports_real_lexical_document() {
    let doc = r#"{"root":{"children":[{"children":[{"detail":0,"format":3,"mode":"normal","style":"","text":"Hi","type":"text","version":1}],"direction":"ltr","format":"center","indent":0,"type":"paragraph","version":1},
      {"children":[{"children":[{"detail":0,"format":0,"mode":"normal","style":"","text":"item","type":"text","version":1}],"direction":"ltr","format":"","indent":0,"type":"listitem","value":1,"version":1}],"direction":"ltr","format":"","indent":0,"listType":"bullet","start":1,"tag":"ul","type":"list","version":1}],
      "direction":"ltr","format":"","indent":0,"type":"root","version":1}}"#;
    let s = EditorState::from_json_str(doc).unwrap();
    let l = Layout::build(&s);
    assert_eq!(l.text, "Hi\n• item");
    assert_eq!(l.runs[0].format, TextFormat::BOLD | TextFormat::ITALIC);
    assert_eq!(l.lines[0].align, Align::Center);
}

#[test]
fn layout_point_mapping_round_trips() {
    let mut e = editor_with("ab\ncd");
    e.dispatch(Command::SelectAll);
    e.dispatch(Command::ToggleList(ListType::Bullet));
    let s = e.state();
    let l = Layout::build(s);
    assert_eq!(l.text, "• ab\n• cd");
    for off in [2usize, 3, 4, 7, 9] {
        let p = l.point_at(s, off);
        assert_eq!(l.offset_of(s, &p), off, "offset {off}");
    }
    // offsets inside the marker clamp to content start
    let p = l.point_at(s, 0);
    assert_eq!(l.offset_of(s, &p), 2);
}

#[test]
fn selected_text_and_line_break() {
    let mut e = editor_with("one\ntwo");
    select(&mut e, 1, 6);
    assert_eq!(e.state().selected_text(), "ne\ntw");
    e.dispatch(Command::InsertLineBreak);
    assert_eq!(lines(&e), ["o\no"]);
}
