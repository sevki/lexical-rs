//! The host logic with no WebAssembly runtime at all: plugins written against the
//! `lexical-plugin` SDK run in this process through `InProcess`. These tests are the proof
//! that nothing in `lexical-plugin-host` depends on a particular runtime.

use lexical_core::{BlockStyle, Command, Editor, HeadingTag, Layout};
use lexical_plugin::{
    BlockKind, Command as PluginCommand, CommandContext, HeadingTag as PluginHeading, Op, Outcome, Plugin, PluginInfo,
    TextContext,
};
use lexical_plugin_host::{ComponentPlugin, InProcess};
use std::cell::Cell;
use std::rc::Rc;

/// Handles three custom commands, like `plugins/command-fixture`.
struct Native;

impl Plugin for Native {
    fn info() -> PluginInfo {
        PluginInfo { name: "native".into(), priority: i32::MIN, transforms_text: true }
    }

    fn handle_command(cmd: PluginCommand, _ctx: CommandContext) -> Outcome {
        let PluginCommand::Custom(name) = cmd else { return Outcome { handled: false, ops: vec![] } };
        match name.as_str() {
            "heading" => Outcome { handled: true, ops: vec![Op::SetBlock(BlockKind::Heading(PluginHeading::H1))] },
            "chain" => Outcome { handled: true, ops: vec![Op::Dispatch(PluginCommand::Custom("ping".into()))] },
            _ => Outcome { handled: false, ops: vec![] },
        }
    }

    // "> " at the start of a paragraph becomes a quote
    fn transform_text(ctx: TextContext) -> Vec<Op> {
        match (ctx.starts_paragraph, ctx.caret) {
            (true, Some(2)) if ctx.text.starts_with("> ") => vec![Op::DeletePrefix(2), Op::SetBlock(BlockKind::Quote)],
            _ => vec![],
        }
    }
}

fn editor() -> Editor {
    let mut e = Editor::new();
    e.add_plugin(Box::new(ComponentPlugin::new(InProcess::<Native>::default()).unwrap()));
    e
}

fn type_str(e: &mut Editor, s: &str) {
    for ch in s.chars() {
        e.dispatch(Command::InsertText(ch.to_string()));
    }
}

fn styles(e: &Editor) -> Vec<BlockStyle> {
    Layout::build(e.state()).lines.iter().map(|l| l.style.clone()).collect()
}

#[test]
fn a_text_transform_runs_the_plugin_and_applies_its_ops() {
    let mut e = editor();
    type_str(&mut e, "> quoted");
    assert!(matches!(styles(&e)[0], BlockStyle::Quote));
    assert_eq!(e.state().to_plain_text(), "quoted");
    e.state().check_invariants().unwrap();
}

#[test]
fn a_command_is_handled_by_the_plugin() {
    let mut e = editor();
    type_str(&mut e, "title");
    assert!(e.dispatch(Command::Custom("heading".into())));
    assert!(matches!(styles(&e)[0], BlockStyle::Heading(HeadingTag::H1)));
    assert!(!e.dispatch(Command::Custom("unknown".into())), "unknown commands fall through");
}

#[test]
fn a_read_only_editor_never_consults_the_plugin() {
    let mut e = editor();
    type_str(&mut e, "title");
    e.set_editable(false);
    assert!(!e.dispatch(Command::Custom("heading".into())));
    assert!(matches!(styles(&e)[0], BlockStyle::Paragraph));
}

#[test]
fn a_command_the_plugin_dispatches_reaches_other_handlers() {
    let mut e = editor();
    let pings = Rc::new(Cell::new(0));
    let seen = pings.clone();
    e.register_command(0, move |_, cmd| {
        if *cmd == Command::Custom("ping".into()) {
            seen.set(seen.get() + 1);
            return true;
        }
        false
    });
    assert!(e.dispatch(Command::Custom("chain".into())));
    assert_eq!(pings.get(), 1);
}
