//! The real `markdown-shortcuts` plugin component, loaded into a real editor.
//!
//! The guest is built on first use (`cargo build --target wasm32-wasip2`), so these tests
//! need that target: `rustup target add wasm32-wasip2`.

use lexical_core::{BlockStyle, Command, Editor, HeadingTag, Layout, ListType, MarkdownShortcutsPlugin};
use lexical_plugin_host::{Budget, PluginError, WasmPlugin};
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

fn plugin() -> WasmPlugin {
    WasmPlugin::load(&std::fs::read(guest()).unwrap()).unwrap()
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
fn the_component_and_the_native_plugin_agree() {
    let inputs = ["# a", "###### six", "> quote", "``` code", "- x", "* y", "[ ] todo", "1. one", "#no space", "a # b", "  # indented"];
    for input in inputs {
        let mut native = Editor::new();
        native.add_plugin(Box::new(MarkdownShortcutsPlugin::default()));
        let mut wasm = Editor::new();
        wasm.add_plugin(Box::new(plugin()));
        type_str(&mut native, input);
        type_str(&mut wasm, input);
        assert_eq!(wasm.state().to_json(), native.state().to_json(), "typing {input:?}");
        wasm.state().check_invariants().unwrap();
    }
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
    assert!(matches!(WasmPlugin::load(b"not wasm"), Err(PluginError::Load(_))));
    // a valid component of the wrong shape
    let empty = wat_component_without_exports();
    assert!(matches!(WasmPlugin::load(&empty), Err(PluginError::Load(_))));
}

fn wat_component_without_exports() -> Vec<u8> {
    // the 8-byte header of an empty component
    vec![0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00]
}

#[test]
fn a_plugin_that_runs_out_of_fuel_fails_the_call_and_not_the_editor() {
    let bytes = std::fs::read(guest()).unwrap();
    let p = WasmPlugin::load_with(&bytes, Budget { fuel: 100, ..Budget::default() }).unwrap();
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
    let r = WasmPlugin::load_with(&bytes, Budget { memory_bytes: 1024, ..Budget::default() });
    match r {
        Err(PluginError::Load(m)) => assert!(m.contains("limit") || m.contains("memory"), "{m}"),
        _ => panic!("a 1 KiB memory cap should refuse the plugin"),
    }
}
