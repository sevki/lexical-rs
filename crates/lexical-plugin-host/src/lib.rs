//! Run editor plugins that are WebAssembly components, on any runtime.
//!
//! A plugin implements the `lexical:editor/plugin` WIT interface (see the `lexical-plugin`
//! crate) and is loaded by a *backend*, the part that depends on a particular WebAssembly
//! runtime: `lexical-wasmtime` for wasmtime, or your own implementation of
//! [`PluginBackend`] for another. This crate holds everything else, so it has no runtime
//! dependency and builds for `wasm32-unknown-unknown`: it can itself run inside Deno or a
//! worker, with the plugins called through the JavaScript host's own WebAssembly support.
//!
//! * [`ComponentPlugin`]: the fine-grained interface, a pure function from a command or a
//!   text node to a list of operations;
//! * [`ComponentDocumentPlugin`]: the coarser document-in, document-out interface;
//! * [`InProcess`]: run a plugin written with the SDK natively, with no runtime at all.
//!
//! Plugins hold no handle to the editor, and a read-only editor never consults them.

mod backend;
mod convert;
mod document;
mod error;
mod ops;
mod plugin;

pub use backend::{DocumentBackend, InProcess, InProcessDocument, PluginBackend};
pub use document::ComponentDocumentPlugin;
pub use error::{PluginError, Result};
pub use plugin::ComponentPlugin;
