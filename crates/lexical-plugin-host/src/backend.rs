//! What a WebAssembly runtime has to provide. Everything else, meaning how commands become
//! plugin calls and how answers become editor updates, lives in this crate and does not
//! depend on any runtime, so the same host logic runs on wasmtime, in a JavaScript host
//! (Deno, workers) or wherever a component can be called.
//!
//! The traits use plain Rust types (the `lexical-plugin` SDK's own), take `&mut self` and
//! are neither `Send` nor `Sync`: an implementation owns its instance and calls it
//! synchronously. Resource limits, such as fuel or memory caps, are the backend's job; a
//! call that exceeds them returns [`PluginError::Call`].

use crate::error::Result;
use lexical_plugin::document::{DocumentOutcome, DocumentPlugin, DocumentSelection};
use lexical_plugin::{Command, CommandContext, Op, Outcome, Plugin, PluginInfo, TextContext};
use std::marker::PhantomData;

/// A loaded component implementing the WIT interface `lexical:editor/plugin`.
pub trait PluginBackend {
    fn info(&mut self) -> Result<PluginInfo>;
    fn handle_command(&mut self, cmd: &Command, ctx: &CommandContext) -> Result<Outcome>;
    fn transform_text(&mut self, ctx: &TextContext) -> Result<Vec<Op>>;
}

/// A loaded component implementing the WIT interface `lexical:editor/document-plugin`.
pub trait DocumentBackend {
    fn run(
        &mut self,
        state: &str,
        selection: Option<DocumentSelection>,
        command: &str,
        payload: &str,
    ) -> Result<DocumentOutcome>;
}

/// Runs a plugin written against the `lexical-plugin` SDK directly in this process, with no
/// WebAssembly at all. It is how the host logic is tested without a runtime, and a way to
/// ship a plugin natively where no Wasm runtime is available.
pub struct InProcess<P>(PhantomData<P>);

impl<P> Default for InProcess<P> {
    fn default() -> Self {
        InProcess(PhantomData)
    }
}

impl<P: Plugin> PluginBackend for InProcess<P> {
    fn info(&mut self) -> Result<PluginInfo> {
        Ok(P::info())
    }

    fn handle_command(&mut self, cmd: &Command, ctx: &CommandContext) -> Result<Outcome> {
        Ok(P::handle_command(cmd.clone(), ctx.clone()))
    }

    fn transform_text(&mut self, ctx: &TextContext) -> Result<Vec<Op>> {
        Ok(P::transform_text(ctx.clone()))
    }
}

/// [`InProcess`] for document plugins.
pub struct InProcessDocument<P>(PhantomData<P>);

impl<P> Default for InProcessDocument<P> {
    fn default() -> Self {
        InProcessDocument(PhantomData)
    }
}

impl<P: DocumentPlugin> DocumentBackend for InProcessDocument<P> {
    fn run(
        &mut self,
        state: &str,
        selection: Option<DocumentSelection>,
        command: &str,
        payload: &str,
    ) -> Result<DocumentOutcome> {
        Ok(P::run(state.to_string(), selection, command.to_string(), payload.to_string()))
    }
}
