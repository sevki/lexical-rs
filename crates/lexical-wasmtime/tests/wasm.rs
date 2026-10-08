//! The real `markdown-shortcuts` plugin component, loaded into a real editor.
//!
//! The guest is built on first use (`cargo build --target wasm32-wasip2`), so these tests
//! need that target: `rustup target add wasm32-wasip2`.

use lexical_core::{BlockStyle, Command, Editor, HeadingTag, Layout, ListType};
use lexical_wasmtime::{load, load_with, Budget, ComponentPlugin, PluginError, WasmtimePlugin};
use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::sync::OnceLock;

fn guest() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/markdown-shortcuts");
        let status = Process::new(env!("CARGO"))
            .args(["build", "--release", "--target", "wasm32-wasip2", "--quiet"])
            .current_dir(&dir)
            .status()
            .expect("run cargo");
        assert!(status.success(), "building the guest plugin failed (is the wasm32-wasip2 target installed?)");
        dir.join("target/wasm32-wasip2/release/markdown_shortcuts.wasm")
    })
}

fn plugin() -> ComponentPlugin<WasmtimePlugin> {
    load(&std::fs::read(guest()).unwrap()).unwrap()
}

/// The test plugin that handles custom commands (see `plugins/command-fixture`).
fn fixture() -> ComponentPlugin<WasmtimePlugin> {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    let path = WASM.get_or_init(|| {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/command-fixture");
        let status = Process::new(env!("CARGO"))
            .args(["build", "--release", "--target", "wasm32-wasip2", "--quiet"])
            .current_dir(&dir)
            .status()
            .expect("run cargo");
        assert!(status.success(), "building the fixture plugin failed");
        dir.join("target/wasm32-wasip2/release/command_fixture.wasm")
    });
    load(&std::fs::read(path).unwrap()).unwrap()
}

fn styles(e: &Editor) -> Vec<BlockStyle> {
    Layout::build(e.state()).lines.iter().map(|l| l.style.clone()).collect()
}

fn texts(e: &Editor) -> Vec<String> {
    let s = e.state();
    s.line_blocks().iter().map(|&b| s.block_content(b).text).collect()
}

fn type_str(e: &mut Editor, s: &str) {
    for ch in s.chars() {
        e.dispatch(Command::InsertText(ch.to_string()));
    }
}

#[test]
fn the_component_describes_itself() {
    assert_eq!(plugin().name(), "markdown-shortcuts");
}

#[test]
fn typing_a_markdown_prefix_makes_the_block() {
    let mut e = Editor::new();
    e.add_plugin(Box::new(plugin()));
    type_str(&mut e, "## Title");
    assert!(matches!(styles(&e)[0], BlockStyle::Heading(HeadingTag::H2)));
    assert_eq!(texts(&e), ["Title"]);

    e.dispatch(Command::InsertParagraph);
    type_str(&mut e, "- item");
    assert!(matches!(styles(&e)[1], BlockStyle::ListItem { list_type: ListType::Bullet, .. }));
    assert_eq!(texts(&e), ["Title", "item"]);
}

#[test]
fn every_shortcut_makes_its_block_and_leaves_the_rest_of_the_text() {
    use BlockStyle::*;
    type Case = (&'static str, fn(&BlockStyle) -> bool, &'static str);
    let cases: [Case; 11] = [
        ("# a", |s| matches!(s, Heading(HeadingTag::H1)), "a"),
        ("###### six", |s| matches!(s, Heading(HeadingTag::H6)), "six"),
        ("> quote", |s| matches!(s, Quote), "quote"),
        ("``` code", |s| matches!(s, Code), "code"),
        ("- x", |s| matches!(s, ListItem { list_type: ListType::Bullet, .. }), "x"),
        ("* y", |s| matches!(s, ListItem { list_type: ListType::Bullet, .. }), "y"),
        ("[ ] todo", |s| matches!(s, ListItem { list_type: ListType::Check, .. }), "todo"),
        ("1. one", |s| matches!(s, ListItem { list_type: ListType::Number, .. }), "one"),
        // not shortcuts: no space after the marker, or not at the start of the paragraph
        ("#no space", |s| matches!(s, Paragraph), "#no space"),
        ("a # b", |s| matches!(s, Paragraph), "a # b"),
        ("  # indented", |s| matches!(s, Paragraph), "  # indented"),
    ];
    for (input, is_block, text) in cases {
        let mut e = Editor::new();
        e.add_plugin(Box::new(plugin()));
        type_str(&mut e, input);
        assert!(is_block(&styles(&e)[0]), "typing {input:?}: {:?}", styles(&e));
        assert_eq!(texts(&e), [text], "typing {input:?}");
        e.state().check_invariants().unwrap();
    }
}

#[test]
fn random_editing_with_the_plugin_keeps_the_document_valid() {
    use lexical_sync::testing::{random_command, random_selection, Rng};
    let mut e = Editor::new();
    let p = plugin();
    let errors = p.errors();
    e.add_plugin(Box::new(p));
    let mut r = Rng::new(11);
    for step in 0..400 {
        if r.chance(5) {
            random_selection(&mut e, &mut r);
        } else {
            e.dispatch(random_command(&mut r));
        }
        e.state().check_invariants().unwrap_or_else(|m| panic!("step {step}: {m}"));
    }
    assert!(errors.borrow().is_empty(), "{:?}", errors.borrow());
}

#[test]
fn a_healthy_plugin_records_no_errors() {
    let mut e = Editor::new();
    let p = plugin();
    let errors = p.errors();
    e.add_plugin(Box::new(p));
    type_str(&mut e, "# a");
    assert!(matches!(styles(&e)[0], BlockStyle::Heading(_)));
    assert!(errors.borrow().is_empty());
}

#[test]
fn bytes_that_are_not_a_plugin_are_rejected() {
    assert!(matches!(load(b"not wasm"), Err(PluginError::Load(_))));
    // a valid component of the wrong shape
    let empty = wat_component_without_exports();
    assert!(matches!(load(&empty), Err(PluginError::Load(_))));
}

fn wat_component_without_exports() -> Vec<u8> {
    // the 8-byte header of an empty component
    vec![0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00]
}

#[test]
fn a_plugin_that_runs_out_of_fuel_fails_the_call_and_not_the_editor() {
    let bytes = std::fs::read(guest()).unwrap();
    let p = load_with(&bytes, Budget { fuel: 100, ..Budget::default() }).unwrap();
    let errors = p.errors();
    let mut e = Editor::new();
    e.add_plugin(Box::new(p));
    type_str(&mut e, "# hello");
    assert_eq!(texts(&e), ["# hello"], "the plugin did nothing, typing still worked");
    assert!(matches!(styles(&e)[0], BlockStyle::Paragraph));
    assert!(matches!(errors.borrow().first(), Some(PluginError::Call(m)) if m.contains("fuel")), "{:?}", errors.borrow());
    e.state().check_invariants().unwrap();
}

#[test]
fn a_tiny_memory_cap_is_refused_at_load() {
    let bytes = std::fs::read(guest()).unwrap();
    let r = load_with(&bytes, Budget { memory_bytes: 1024, ..Budget::default() });
    match r {
        Err(PluginError::Load(m)) => assert!(m.contains("limit") || m.contains("memory"), "{m}"),
        _ => panic!("a 1 KiB memory cap should refuse the plugin"),
    }
}

#[test]
fn a_plugin_answers_a_custom_command_with_a_block_change() {
    let mut e = Editor::new();
    e.add_plugin(Box::new(fixture()));
    type_str(&mut e, "title");
    assert!(e.dispatch(Command::Custom("heading".into())), "the plugin handled it");
    assert!(matches!(styles(&e)[0], BlockStyle::Heading(HeadingTag::H1)));
    assert!(!e.dispatch(Command::Custom("unknown".into())), "unrecognised commands fall through");
}

#[test]
fn a_read_only_editor_never_consults_plugins() {
    let mut e = Editor::new();
    e.add_plugin(Box::new(fixture()));
    type_str(&mut e, "title");
    e.set_editable(false);
    assert!(!e.dispatch(Command::Custom("heading".into())));
    assert!(matches!(styles(&e)[0], BlockStyle::Paragraph), "the plugin could not change a read-only document");
}

#[test]
fn a_command_a_plugin_dispatches_reaches_the_other_handlers() {
    use std::cell::Cell;
    use std::rc::Rc;
    let mut e = Editor::new();
    e.add_plugin(Box::new(fixture())); // priority i32::MIN: runs after the handler below
    let pings = Rc::new(Cell::new(0));
    let seen = pings.clone();
    e.register_command(0, move |_, cmd| {
        if *cmd == Command::Custom("ping".into()) {
            seen.set(seen.get() + 1);
            return true;
        }
        false
    });
    // the plugin's own handler is reached by "chain" and dispatches "ping" while running
    assert!(e.dispatch(Command::Custom("chain".into())));
    assert_eq!(pings.get(), 1, "the application handler saw the plugin's dispatched command");
}

#[test]
fn the_lowest_possible_priority_is_accepted() {
    // i32::MIN used to overflow when handlers were sorted by negated priority
    let mut e = Editor::new();
    e.add_plugin(Box::new(fixture()));
    e.register_command(i32::MAX, |_, _| false);
    assert!(e.dispatch(Command::Custom("heading".into())));
}

#[test]
fn recorded_failures_are_bounded() {
    let mut e = Editor::new();
    let p = fixture();
    let errors = p.errors();
    e.add_plugin(Box::new(p));
    for _ in 0..500 {
        e.dispatch(Command::Custom("crash".into()));
    }
    let n = errors.borrow().len();
    assert!(n > 0 && n <= 64, "{n} failures kept");
    e.state().check_invariants().unwrap();
}
