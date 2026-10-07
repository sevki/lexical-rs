//! The flat interchange form: round trips, idempotence, and totality of `unflatten`.

use lexical_core::{
    Align, BlockType, Command, Editor, HeadingTag, Layout, ListType, TextFormat,
};
use lexical_sync::flat::{BlockKind, CharAttr, Flat, Inline, Line};
use lexical_sync::marks::{self, MarkValue, MAX_DEPTH};
use lexical_sync::testing::{random_command, random_selection, Rng};
use lexical_sync::{flatten, unflatten};

fn select(e: &mut Editor, from: usize, to: usize) {
    let s = e.state();
    let l = Layout::build(s);
    let (a, f) = (l.point_at(s, from), l.point_at(s, to));
    e.set_selection(a, f);
}

/// What a user can see of a document: text, per-line block styling, per-run formatting.
fn appearance(e: &lexical_core::EditorState) -> String {
    let l = Layout::build(e);
    format!("{:?}\n{:?}\n{:?}", l.text, l.lines.iter().map(|x| (&x.style, x.align, x.indent)).collect::<Vec<_>>(),
        l.runs.iter().map(|r| (r.start, r.end, r.format, &r.link)).collect::<Vec<_>>())
}

#[test]
fn a_rich_document_round_trips() {
    let mut e = Editor::new();
    e.dispatch(Command::Paste(
        "Title\nbold italic link\nitem one\nitem two\nnested\nstep\ntodo\nquote\ncode".into(),
    ));
    select(&mut e, 0, 0);
    e.dispatch(Command::SetBlockType(BlockType::Heading(HeadingTag::H2)));
    e.dispatch(Command::FormatAlign(Align::Center));
    select(&mut e, 6, 10);
    e.dispatch(Command::FormatText(TextFormat::BOLD));
    select(&mut e, 11, 17);
    e.dispatch(Command::FormatText(TextFormat::ITALIC));
    e.dispatch(Command::ToggleLink(Some("https://lexical.dev".into())));
    select(&mut e, 24, 33);
    e.dispatch(Command::ToggleList(ListType::Bullet));
    select(&mut e, 44, 50);
    e.dispatch(Command::Indent);
    select(&mut e, 51, 55);
    e.dispatch(Command::ToggleList(ListType::Number));
    select(&mut e, 61, 65);
    e.dispatch(Command::ToggleList(ListType::Check));
    e.dispatch(Command::ToggleCheck);
    e.state().check_invariants().unwrap();

    let flat = flatten(e.state());
    let back = unflatten(&flat);
    back.check_invariants().unwrap();
    assert_eq!(appearance(&back), appearance(e.state()));
    assert_eq!(flatten(&back), flat, "flatten(unflatten(f)) == f");
}

#[test]
fn flatten_is_stable_over_random_editing_sessions() {
    for seed in 1..=150u64 {
        let mut r = Rng::new(seed);
        let mut e = Editor::new();
        for step in 0..80 {
            if r.chance(4) {
                random_selection(&mut e, &mut r);
            } else {
                e.dispatch(random_command(&mut r));
            }
            let flat = flatten(e.state());
            let back = unflatten(&flat);
            if let Err(m) = back.check_invariants() {
                panic!("seed {seed} step {step}: {m}");
            }
            assert_eq!(flatten(&back), flat, "seed {seed} step {step}");
            assert_eq!(appearance(&back), appearance(e.state()), "seed {seed} step {step}");
        }
    }
}

fn random_flat(r: &mut Rng) -> Flat {
    let mut f = Flat::default();
    let n = r.below(60);
    for _ in 0..n {
        match r.below(10) {
            0..=4 => {
                let ch = ['a', 'b', 'é', '👍', ' '][r.below(5)];
                let inline = Inline {
                    format: TextFormat::from_bits_truncate(r.below(256) as u32),
                    link: r.chance(5).then(|| ["u1", "u2"][r.below(2)].to_string()),
                    style: if r.chance(6) { "color:red".into() } else { String::new() },
                };
                f.push_inline(ch, inline);
            }
            5 => f.push_soft(),
            _ => {
                let kind = match r.below(8) {
                    0 => BlockKind::Heading(HeadingTag::H3),
                    1 => BlockKind::Quote,
                    2 => BlockKind::Code,
                    3 | 4 => BlockKind::ListItem(ListType::Bullet),
                    5 => BlockKind::ListItem(ListType::Number),
                    6 => BlockKind::ListItem(ListType::Check),
                    _ => BlockKind::Paragraph,
                };
                f.push_end(Line {
                    kind,
                    // depths jump around, including far deeper than the previous item
                    depth: if r.chance(4) { r.below(12) as u32 } else { r.below(3) as u32 },
                    align: [Align::Start, Align::Center, Align::Right][r.below(3)],
                    indent: r.below(4) as u32,
                    checked: Some(r.chance(2)),
                });
            }
        }
    }
    f
}

#[test]
fn arbitrary_flat_documents_always_become_valid_trees() {
    // A CRDT merge can produce any combination of characters and attributes; rebuilding a
    // document must never panic or violate an invariant, whatever the merge result is.
    for seed in 1..=3000u64 {
        let flat = random_flat(&mut Rng::new(seed));
        let state = unflatten(&flat);
        if let Err(m) = state.check_invariants() {
            panic!("seed {seed}: {m}\nflat: {:?}", flat.text());
        }
        // and the rebuilt document is itself a fixed point
        let again = unflatten(&flatten(&state));
        assert_eq!(flatten(&again), flatten(&state), "seed {seed}: not a fixed point");
    }
}

#[test]
fn missing_final_terminator_still_makes_a_valid_document() {
    let mut f = Flat::default();
    f.push_inline('a', Inline::default());
    f.push_end(Line::default());
    f.push_inline('b', Inline::default()); // no terminator
    let s = unflatten(&f);
    s.check_invariants().unwrap();
    assert_eq!(s.to_plain_text(), "a\n\nb");
    assert_eq!(unflatten(&Flat::default()).to_plain_text(), "");
}

#[test]
fn hostile_marks_are_clamped() {
    let get = |depth: i64| {
        move |key: &str| match key {
            "list" => Some(MarkValue::Str("bullet".into())),
            "depth" => Some(MarkValue::Int(depth)),
            "indent" => Some(MarkValue::Int(-5)),
            _ => None,
        }
    };
    let CharAttr::End(line) = marks::decode('\n', &get(i64::MAX)) else { panic!() };
    assert_eq!(line.depth, MAX_DEPTH);
    assert_eq!(line.indent, 0, "negative indent is clamped to zero");
    let CharAttr::End(line) = marks::decode('\n', &get(-3)) else { panic!() };
    assert_eq!(line.depth, 0);

    // a document with the maximum depth must still build, quickly, into a valid tree
    let mut f = Flat::default();
    f.push_inline('x', Inline::default());
    f.push_end(Line { kind: BlockKind::ListItem(ListType::Bullet), depth: MAX_DEPTH, ..Line::default() });
    let s = unflatten(&f);
    s.check_invariants().unwrap();
    // unknown or malformed values fall back to defaults instead of failing
    let weird = |key: &str| (key == "block").then_some(MarkValue::Int(7));
    let CharAttr::End(line) = marks::decode('\n', &weird) else { panic!() };
    assert_eq!(line.kind, BlockKind::Paragraph);
}

#[test]
fn literal_newlines_and_line_separators_inside_text_become_soft_breaks() {
    use lexical_core::{EditorState, NodeData, ROOT_KEY};
    let mut s = EditorState::empty();
    let p = s.create_paragraph();
    s.append_child(ROOT_KEY, p);
    let t = s.create_node(NodeData::text("a\nb\u{2028}c", TextFormat::empty()));
    s.append_child(p, t);
    let flat = flatten(&s);
    assert_eq!(flat.text(), "a\u{2028}b\u{2028}c\n");
    unflatten(&flat).check_invariants().unwrap();
}
