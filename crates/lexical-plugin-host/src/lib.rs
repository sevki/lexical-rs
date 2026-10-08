//! Run editor plugins that are WebAssembly components.
//!
//! A plugin implements the `lexical:editor/plugin` WIT interface (see the `lexical-plugin`
//! crate) and is loaded with [`WasmPlugin::load`]. It receives a description of a command
//! or a text node and answers with operations; it never touches the editor itself, and it
//! runs under a fuel budget and a memory cap, so a faulty or hostile plugin cannot hang or
//! exhaust the host.

mod bindings;
mod convert;
mod document;
mod document_bindings;
mod error;
mod ops;
mod runtime;
mod sandbox;

pub use document::WasmDocumentPlugin;
pub use error::{PluginError, Result};
pub use runtime::WasmPlugin;
pub use sandbox::Budget;
