//! Lexical for JavaScript, running inside a component behind `plugins/lexical-js-shim`.
//!
//! The component is built on first use (`npm install && npm run build` in the shim's
//! directory), so these tests need node and network access to the npm registry.

use lexical_core::{BlockStyle, Command, Editor, HeadingTag, Layout, ListType, TextFormat};
use lexical_plugin_wasmtime::{load_document, load_document_with, Budget, ComponentDocumentPlugin, PluginError, WasmtimeDocument};
use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::sync::OnceLock;
use std::time::Instant;

fn component() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/lexical-js-shim");
        let run = |args: &[&str]| {
            let status = Process::new("npm").args(args).current_dir(&dir).status().expect("run npm");
            assert!(status.success(), "npm {args:?} failed");
        };
        if !dir.join("node_modules").exists() {
            run(&["ci", "--silent"]);
        }
        run(&["run", "build", "--silent"]);
        dir.join("dist/lexical-js-shim.wasm")
    })
}

fn plugin() -> ComponentDocumentPlugin<WasmtimeDocument> {
    load_document(&std::fs::read(component()).unwrap()).unwrap()
}

fn type_str(e: &mut Editor, s: &str) {
    for ch in s.chars() {
        e.dispatch(Command::InsertText(ch.to_string()));
    }
}

fn texts(e: &Editor) -> Vec<String> {
    let s = e.state();
    s.line_blocks().iter().map(|&b| s.block_content(b).text).collect()
}

fn styles(e: &Editor) -> Vec<BlockStyle> {
    Layout::build(e.state()).lines.iter().map(|l| l.style.clone()).collect()
}

#[test]
fn the_real_markdown_plugin_from_lexical_js_makes_a_heading() {
    let mut e = Editor::new();
    let p = plugin();
    let errors = p.errors();
    e.add_plugin(Box::new(p));
    let started = Instant::now();
    type_str(&mut e, "## Hi");
    eprintln!("5 keystrokes through Lexical JS in a component: {:?}", started.elapsed());
    assert!(errors.borrow().is_empty(), "{:?}", errors.borrow());
    assert!(matches!(styles(&e)[0], BlockStyle::Heading(HeadingTag::H2)), "{:?}", styles(&e));
    assert_eq!(texts(&e), ["Hi"]);
    e.state().check_invariants().unwrap();

    // and a list, after Enter
    e.dispatch(Command::InsertParagraph);
    type_str(&mut e, "- item");
    assert!(matches!(styles(&e).last(), Some(BlockStyle::ListItem { list_type: ListType::Bullet, .. })));
    assert_eq!(texts(&e), ["Hi", "item"]);
}

#[test]
fn plain_editing_through_lexical_js_matches_the_native_engine() {
    let script = |e: &mut Editor| {
        type_str(e, "hello world");
        e.dispatch(Command::InsertParagraph);
        type_str(e, "second");
        e.dispatch(Command::DeleteCharacter { backward: true });
        e.dispatch(Command::DeleteCharacter { backward: true });
        type_str(e, "!");
    };
    let mut native = Editor::new();
    script(&mut native);
    let mut js = Editor::new();
    js.add_plugin(Box::new(plugin()));
    script(&mut js);
    assert_eq!(texts(&js), texts(&native));
    assert_eq!(texts(&js), ["hello world", "seco!"]);
    js.state().check_invariants().unwrap();
}

#[test]
fn formatting_goes_through_lexical_js_and_undo_stays_native() {
    let mut e = Editor::new();
    e.add_plugin(Box::new(plugin()));
    type_str(&mut e, "bold");
    e.dispatch(Command::SelectAll);
    e.dispatch(Command::FormatText(TextFormat::BOLD));
    let json = e.state().to_json();
    assert_eq!(json["root"]["children"][0]["children"][0]["format"], 1, "{json}");
    // Undo is not a command the shim knows, so the editor's own history handles it.
    assert!(e.dispatch(Command::Undo));
    assert_eq!(e.state().to_json()["root"]["children"][0]["children"][0]["format"], 0);
}

#[test]
fn a_starved_budget_fails_the_call_and_leaves_the_editor_alone() {
    let bytes = std::fs::read(component()).unwrap();
    let p = load_document_with(&bytes, Budget { fuel: 1_000, memory_bytes: 256 << 20 }, 0).unwrap();
    let errors = p.errors();
    let mut e = Editor::new();
    e.add_plugin(Box::new(p));
    type_str(&mut e, "ab");
    assert_eq!(texts(&e), ["ab"], "the editor's own typing took over");
    assert!(matches!(errors.borrow().first(), Some(PluginError::Call(m)) if m.contains("fuel")), "{:?}", errors.borrow());
}
