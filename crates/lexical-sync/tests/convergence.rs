//! Concurrent editing: whatever two or more peers do at the same time, once they have
//! exchanged their updates they hold the same document, it is a valid one, and no peer's
//! text has been lost.

use lexical_core::{BlockStyle, Command, Layout, ListType, TextFormat};
use lexical_sync::testing::{random_command, random_selection, Cluster, Rng};
use lexical_sync::{flatten, Replica, SyncError, BOOTSTRAP_PEER};

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

fn lines(r: &Replica) -> Vec<String> {
    let s = r.editor.state();
    s.line_blocks().iter().map(|&b| s.block_content(b).text).collect()
}

fn finish(c: &mut Cluster) {
    c.sync_all();
    c.assert_valid();
    assert!(c.converged(), "replicas diverged:\n{:?}\n{:?}", lines(&c.replicas[0]), lines(&c.replicas[1]));
}

/// Everyone starts from the same text.
fn shared(n: usize, text: &str) -> Cluster {
    let mut c = Cluster::new(n);
    c.replicas[0].dispatch(Command::Paste(text.into()));
    c.sync_all();
    assert!(c.converged());
    c
}

#[test]
fn replicas_start_from_one_shared_empty_paragraph() {
    let mut c = Cluster::new(3);
    assert!(c.converged());
    c.sync_all();
    assert!(c.converged());
    for r in &c.replicas {
        assert_eq!(r.editor.state().line_blocks().len(), 1, "no duplicate first paragraph");
    }
}

#[test]
fn typing_at_the_same_caret_keeps_both_edits() {
    let mut c = shared(2, "abcd");
    place(&mut c.replicas[0], 2);
    place(&mut c.replicas[1], 2);
    c.replicas[0].dispatch(Command::InsertText("XX".into()));
    c.replicas[1].dispatch(Command::InsertText("YY".into()));
    finish(&mut c);
    let t = lines(&c.replicas[0]).join("\n");
    assert!(t == "abXXYYcd" || t == "abYYXXcd", "{t}");
}

#[test]
fn enter_and_typing_elsewhere_in_the_same_line() {
    let mut c = shared(2, "hello world");
    place(&mut c.replicas[0], 5);
    place(&mut c.replicas[1], 11);
    c.replicas[0].dispatch(Command::InsertParagraph);
    c.replicas[1].dispatch(Command::InsertText("!".into()));
    finish(&mut c);
    assert_eq!(lines(&c.replicas[0]), ["hello", " world!"]);
}

#[test]
fn both_peers_press_enter_at_the_same_spot() {
    let mut c = shared(2, "ab");
    place(&mut c.replicas[0], 1);
    place(&mut c.replicas[1], 1);
    c.replicas[0].dispatch(Command::InsertParagraph);
    c.replicas[1].dispatch(Command::InsertParagraph);
    finish(&mut c);
    // each Enter is its own insertion, so the line is split twice and no text is lost
    assert_eq!(lines(&c.replicas[0]).concat(), "ab");
    assert_eq!(lines(&c.replicas[0]).len(), 3);
}

#[test]
fn backspace_merging_two_lines_while_a_peer_edits_the_second() {
    let mut c = shared(2, "ab\ncd");
    place(&mut c.replicas[0], 3); // start of "cd"
    place(&mut c.replicas[1], 4); // between c and d
    c.replicas[0].dispatch(Command::DeleteCharacter { backward: true });
    c.replicas[1].dispatch(Command::InsertText("X".into()));
    finish(&mut c);
    assert_eq!(lines(&c.replicas[0]), ["abcXd"]);
}

#[test]
fn deleting_text_a_peer_is_editing() {
    let mut c = shared(2, "hello brave world");
    select(&mut c.replicas[0], 5, 11); // " brave"
    place(&mut c.replicas[1], 8); // inside "brave"
    c.replicas[0].dispatch(Command::DeleteCharacter { backward: true });
    c.replicas[1].dispatch(Command::InsertText("-NEW-".into()));
    finish(&mut c);
    let t = lines(&c.replicas[0]).join("\n");
    assert!(t.contains("-NEW-") && t.starts_with("hello") && t.ends_with(" world"), "{t}");
}

#[test]
fn formatting_a_range_while_a_peer_types_at_its_end() {
    let mut c = shared(2, "hello");
    select(&mut c.replicas[0], 0, 5);
    place(&mut c.replicas[1], 5);
    c.replicas[0].dispatch(Command::FormatText(TextFormat::BOLD));
    c.replicas[1].dispatch(Command::InsertText("!".into()));
    finish(&mut c);
    let l = Layout::build(c.replicas[0].editor.state());
    assert_eq!(l.text, "hello!");
    let bold: Vec<_> = l.runs.iter().filter(|r| r.format.contains(TextFormat::BOLD)).map(|r| (r.start, r.end)).collect();
    assert_eq!(bold, [(0, 5)], "the bold range did not swallow the concurrently typed '!'");
}

#[test]
fn different_formats_on_the_same_text_both_apply() {
    let mut c = shared(2, "hello");
    select(&mut c.replicas[0], 0, 5);
    select(&mut c.replicas[1], 0, 5);
    c.replicas[0].dispatch(Command::FormatText(TextFormat::BOLD));
    c.replicas[1].dispatch(Command::FormatText(TextFormat::ITALIC));
    finish(&mut c);
    let l = Layout::build(c.replicas[0].editor.state());
    assert_eq!(l.runs.len(), 1);
    assert_eq!(l.runs[0].format, TextFormat::BOLD | TextFormat::ITALIC);
}

#[test]
fn conflicting_block_types_settle_on_one() {
    let mut c = shared(2, "title");
    place(&mut c.replicas[0], 2);
    place(&mut c.replicas[1], 2);
    c.replicas[0].dispatch(Command::SetBlockType(lexical_core::BlockType::Heading(lexical_core::HeadingTag::H1)));
    c.replicas[1].dispatch(Command::SetBlockType(lexical_core::BlockType::Quote));
    finish(&mut c);
    let style = Layout::build(c.replicas[0].editor.state()).lines[0].style.clone();
    assert!(matches!(style, BlockStyle::Heading(_) | BlockStyle::Quote), "{style:?}");
}

#[test]
fn making_a_list_while_a_peer_types_in_it() {
    let mut c = shared(2, "one\ntwo\nthree");
    select(&mut c.replicas[0], 0, 13);
    place(&mut c.replicas[1], 7); // end of "two"
    c.replicas[0].dispatch(Command::ToggleList(ListType::Bullet));
    c.replicas[1].dispatch(Command::InsertText("!".into()));
    finish(&mut c);
    let l = Layout::build(c.replicas[0].editor.state());
    assert_eq!(l.text, "• one\n• two!\n• three");
}

#[test]
fn a_peer_indents_a_list_item_another_is_editing() {
    let mut c = shared(2, "a\nb\nc");
    select(&mut c.replicas[0], 0, 5);
    c.replicas[0].dispatch(Command::ToggleList(ListType::Number));
    c.sync_all();
    // "1. a\n2. b\n3. c": offset 9 is just after the "b" (offsets 5-7 are the item's marker)
    place(&mut c.replicas[0], 9);
    place(&mut c.replicas[1], 9);
    c.replicas[0].dispatch(Command::Indent);
    c.replicas[1].dispatch(Command::InsertText("x".into()));
    finish(&mut c);
    let l = Layout::build(c.replicas[0].editor.state());
    assert!(l.text.contains("bx"));
    assert!(matches!(l.lines[1].style, BlockStyle::ListItem { depth: 1, .. }));
}

#[test]
fn offline_divergence_then_merge() {
    for seed in 1..=40u64 {
        let mut c = Cluster::new(3);
        c.replicas[0].dispatch(Command::Paste("alpha\nbeta\ngamma".into()));
        c.sync_all();
        let mut r = Rng::new(seed);
        for step in 0..75 {
            let who = r.below(3);
            if r.chance(4) {
                random_selection(&mut c.replicas[who].editor, &mut r);
            } else {
                c.replicas[who].dispatch(random_command(&mut r));
            }
            if step % 15 == 0 {
                c.assert_valid();
            }
        }
        c.sync_all();
        c.assert_valid();
        assert!(c.converged(), "seed {seed}");
    }
}

#[test]
fn delivery_order_does_not_matter() {
    let mut c = Cluster::new(3);
    c.replicas[0].dispatch(Command::Paste("shared base".into()));
    c.sync_all();
    for (i, text) in ["A", "B", "C"].iter().enumerate() {
        place(&mut c.replicas[i], 3 + i);
        c.replicas[i].dispatch(Command::InsertText((*text).into()));
        c.replicas[i].dispatch(Command::InsertParagraph);
        c.replicas[i].dispatch(Command::FormatText(TextFormat::BOLD));
        c.replicas[i].dispatch(Command::InsertText(format!("{text}{text}")));
    }
    let all: Vec<Vec<u8>> = (0..3).flat_map(|i| c.take_outbox(i)).collect();
    assert!(all.len() >= 6);

    let mut results = vec![];
    let mut rng = Rng::new(7);
    for _ in 0..24 {
        let mut order: Vec<usize> = (0..all.len()).collect();
        for i in (1..order.len()).rev() {
            order.swap(i, rng.below(i + 1));
        }
        // a fresh peer that has seen only the shared base
        let snapshot = {
            let mut base = Cluster::new(1);
            base.replicas[0].dispatch(Command::Paste("shared base".into()));
            base.replicas[0].collab.snapshot().unwrap()
        };
        let mut fresh = Replica::join(50, &snapshot).unwrap();
        // the shared-base edits came from replica 0 in the cluster; include them first
        for m in &order {
            fresh.receive(&all[*m]).unwrap();
        }
        results.push(flatten(fresh.editor.state()));
    }
    assert!(results.windows(2).all(|w| w[0] == w[1]), "result depended on delivery order");
}

#[test]
fn duplicated_and_reordered_updates_are_harmless() {
    let mut c = Cluster::new(2);
    c.replicas[0].dispatch(Command::Paste("one\ntwo".into()));
    c.replicas[0].dispatch(Command::FormatText(TextFormat::ITALIC));
    c.replicas[0].dispatch(Command::InsertText("three".into()));
    let msgs = c.take_outbox(0);
    let expected = {
        let mut inorder = c.replicas.remove(1);
        for m in &msgs {
            inorder.receive(m).unwrap();
        }
        flatten(inorder.editor.state())
    };

    let mut chaos = Replica::new(2).unwrap();
    let mut rng = Rng::new(11);
    let mut delivered: Vec<&Vec<u8>> = msgs.iter().chain(msgs.iter()).chain(msgs.iter()).collect();
    for i in (1..delivered.len()).rev() {
        delivered.swap(i, rng.below(i + 1));
    }
    for m in delivered {
        chaos.receive(m).unwrap();
        chaos.editor.state().check_invariants().unwrap();
    }
    assert_eq!(flatten(chaos.editor.state()), expected);
}

#[test]
fn dropped_updates_are_recovered_by_anti_entropy() {
    let mut c = Cluster::new(3);
    let mut r = Rng::new(3);
    for _ in 0..60 {
        let who = r.below(3);
        c.replicas[who].dispatch(random_command(&mut r));
        // the network loses most messages
        for to in 0..3 {
            for m in c.take_outbox(who) {
                if to != who && r.chance(6) {
                    c.replicas[to].receive(&m).unwrap();
                }
            }
        }
    }
    c.anti_entropy();
    c.anti_entropy();
    c.assert_valid();
    assert!(c.converged());
}

#[test]
fn a_late_joiner_starts_from_a_snapshot_and_follows_live_edits() {
    let mut c = Cluster::new(2);
    c.replicas[0].dispatch(Command::Paste("# not a heading\nsome history".into()));
    c.replicas[1].dispatch(Command::InsertText("!".into()));
    c.sync_all();
    let snapshot = c.replicas[0].collab.snapshot().unwrap();
    let mut late = Replica::join(9, &snapshot).unwrap();
    assert_eq!(flatten(late.editor.state()), flatten(c.replicas[0].editor.state()));

    // live edits flow both ways afterwards
    late.dispatch(Command::InsertText("late".into()));
    for m in late.drain_updates() {
        c.replicas[0].receive(&m).unwrap();
        c.replicas[1].receive(&m).unwrap();
    }
    c.replicas[0].dispatch(Command::InsertText("early".into()));
    for m in c.take_outbox(0) {
        late.receive(&m).unwrap();
    }
    assert_eq!(flatten(late.editor.state()), flatten(c.replicas[0].editor.state()));
}

#[test]
fn a_stale_replica_catches_up_from_a_version_vector() {
    let mut c = Cluster::new(2);
    c.replicas[0].dispatch(Command::InsertText("first".into()));
    c.sync_all();
    // replica 1 goes offline; replica 0 keeps editing
    for i in 0..10 {
        c.replicas[0].dispatch(Command::InsertText(format!(" {i}")));
    }
    c.take_outbox(0);
    let behind = c.replicas[1].collab.version();
    let diff = c.replicas[0].collab.updates_since(&behind).unwrap();
    c.replicas[1].receive(&diff).unwrap();
    assert_eq!(flatten(c.replicas[1].editor.state()), flatten(c.replicas[0].editor.state()));
}

#[test]
fn the_reserved_bootstrap_peer_id_is_rejected() {
    assert_eq!(Replica::new(BOOTSTRAP_PEER).err(), Some(SyncError::ReservedPeer));
}

#[test]
fn garbage_updates_are_rejected_without_corrupting_the_document() {
    let mut r = Replica::new(1).unwrap();
    r.dispatch(Command::InsertText("safe".into()));
    let before = flatten(r.editor.state());
    for junk in [&b""[..], b"not a loro update", &[0xff; 64]] {
        assert!(r.receive(junk).is_err(), "{junk:?}");
    }
    assert_eq!(flatten(r.editor.state()), before);
    r.editor.state().check_invariants().unwrap();
}
