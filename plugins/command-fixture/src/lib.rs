//! A plugin used by the host's tests. It understands three custom commands:
//!
//! * `heading`: turn the caret's block into a heading;
//! * `chain`: dispatch the custom command `ping`, which the host's application handles;
//! * `crash`: trap.

use lexical_plugin::{
    BlockKind, Command, CommandContext, HeadingTag, Op, Outcome, Plugin, PluginInfo, TextContext,
};

struct Fixture;

impl Plugin for Fixture {
    fn info() -> PluginInfo {
        // The lowest priority a component can ask for.
        PluginInfo { name: "command-fixture".into(), priority: i32::MIN, transforms_text: false }
    }

    fn handle_command(cmd: Command, _ctx: CommandContext) -> Outcome {
        let Command::Custom(name) = cmd else { return Outcome { handled: false, ops: vec![] } };
        match name.as_str() {
            "heading" => Outcome { handled: true, ops: vec![Op::SetBlock(BlockKind::Heading(HeadingTag::H1))] },
            "chain" => Outcome { handled: true, ops: vec![Op::Dispatch(Command::Custom("ping".into()))] },
            "crash" => panic!("asked to crash"),
            _ => Outcome { handled: false, ops: vec![] },
        }
    }

    fn transform_text(_ctx: TextContext) -> Vec<Op> {
        vec![]
    }
}

lexical_plugin::export_plugin!(Fixture);
