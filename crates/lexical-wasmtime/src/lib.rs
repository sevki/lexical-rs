//! wasmtime backend for `lexical-plugin-host`.
//!
//! [`load`] and [`load_document`] compile a plugin component, instantiate it in a sandbox
//! (an empty WASI context, a memory cap, a fuel budget per call) and return a plugin ready
//! for `Editor::add_plugin`. All editor logic is in the runtime-neutral host crate; this
//! crate is only the wasmtime part, so another runtime needs its own small backend, not a
//! fork of the host.

mod backend;
mod bindings;
mod convert;
mod document_bindings;
mod sandbox;

pub use backend::{WasmtimeDocument, WasmtimePlugin};
pub use lexical_plugin_host::{ComponentDocumentPlugin, ComponentPlugin, PluginError, Result};
pub use sandbox::Budget;

/// Load a plugin component (`lexical:editor/plugin`) with the default [`Budget`].
pub fn load(bytes: &[u8]) -> Result<ComponentPlugin<WasmtimePlugin>> {
    load_with(bytes, Budget::default())
}

pub fn load_with(bytes: &[u8], budget: Budget) -> Result<ComponentPlugin<WasmtimePlugin>> {
    ComponentPlugin::new(WasmtimePlugin::new(bytes, budget)?)
}

/// Load a document plugin component (`lexical:editor/document-plugin`). The default budget
/// is sized for a JavaScript runtime such as the Lexical JS shim, far more than a small
/// native plugin needs; see [`load_document_with`].
pub fn load_document(bytes: &[u8]) -> Result<ComponentDocumentPlugin<WasmtimeDocument>> {
    load_document_with(bytes, Budget { fuel: 5_000_000_000, memory_bytes: 256 << 20 }, 0)
}

/// `priority`: higher runs first; the editor's own behaviour runs after every plugin.
pub fn load_document_with(
    bytes: &[u8],
    budget: Budget,
    priority: i32,
) -> Result<ComponentDocumentPlugin<WasmtimeDocument>> {
    Ok(ComponentDocumentPlugin::new(WasmtimeDocument::new(bytes, budget)?, priority))
}
