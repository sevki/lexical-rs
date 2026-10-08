//! Write a Lexical editor plugin as a WebAssembly component.
//!
//! A plugin is a pure function from a description of a command or a text node to a list of
//! operations for the editor to perform (`wit/lexical.wit`). Implement [`Plugin`] and
//! export it with [`export_plugin!`]:
//!
//! ```ignore
//! use lexical_plugin::{Command, CommandContext, Op, Outcome, Plugin, PluginInfo, TextContext};
//!
//! struct Shout;
//!
//! impl Plugin for Shout {
//!     fn info() -> PluginInfo { /* name, priority, transforms_text */ }
//!     fn handle_command(cmd: Command, ctx: CommandContext) -> Outcome { /* ... */ }
//!     fn transform_text(ctx: TextContext) -> Vec<Op> { vec![] }
//! }
//!
//! lexical_plugin::export_plugin!(Shout);
//! ```
//!
//! Build with `cargo build --release --target wasm32-wasip2` and load the `.wasm` with
//! a backend such as `lexical-wasmtime` (the runtime-neutral logic is in
//! `lexical-plugin-host`).

pub mod bindings {
    wit_bindgen::generate!({
        path: "wit",
        world: "lexical-plugin",
        pub_export_macro: true,
        export_macro_name: "export_impl",
        default_bindings_module: "lexical_plugin::bindings",
    });
}

/// Export a type implementing [`Plugin`] as the component's `lexical:editor/plugin`.
#[macro_export]
macro_rules! export_plugin {
    ($plugin:ident) => {
        $crate::bindings::export_impl!($plugin with_types_in $crate::bindings);
    };
}

pub use bindings::exports::lexical::editor::plugin::Guest as Plugin;
pub use bindings::lexical::editor::types::*;

/// The coarser `document-plugin` interface: the plugin receives the whole document (Lexical
/// JSON) and returns the whole document. See `wit/lexical.wit`.
pub mod document {
    pub mod bindings {
        wit_bindgen::generate!({
            path: "wit",
            world: "lexical-document-plugin",
            pub_export_macro: true,
            export_macro_name: "export_document_impl",
            default_bindings_module: "lexical_plugin::document::bindings",
        });
    }

    pub use bindings::exports::lexical::editor::document_plugin::{
        DocumentOutcome, DocumentSelection, Guest as DocumentPlugin, Position,
    };
}

/// Export a type implementing [`document::DocumentPlugin`] as the component's
/// `lexical:editor/document-plugin`.
#[macro_export]
macro_rules! export_document_plugin {
    ($plugin:ident) => {
        $crate::document::bindings::export_document_impl!($plugin with_types_in $crate::document::bindings);
    };
}
