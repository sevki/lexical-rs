//! The two backends: a plugin component instantiated in a wasmtime store.

use crate::bindings::LexicalPlugin;
use crate::convert::{
    command_context_to, command_to, document_outcome_from, info_from, ops_from, outcome_from, selection_to,
    text_context_to,
};
use crate::document_bindings::LexicalDocumentPlugin;
use crate::sandbox::{arm, call_error, load_error, prepare, Budget, HostState};
use lexical_plugin as sdk;
use lexical_plugin::document::{DocumentOutcome, DocumentSelection};
use lexical_plugin_host::{DocumentBackend, PluginBackend, PluginError, Result};
use wasmtime::Store;

/// A `lexical:editor/plugin` component in a wasmtime store.
pub struct WasmtimePlugin {
    store: Store<HostState>,
    plugin: LexicalPlugin,
    budget: Budget,
}

impl WasmtimePlugin {
    pub fn new(bytes: &[u8], budget: Budget) -> Result<WasmtimePlugin> {
        let mut sandbox = prepare(bytes, budget)?;
        let plugin = LexicalPlugin::instantiate(&mut sandbox.store, &sandbox.component, &sandbox.linker)
            .map_err(load_error)?;
        Ok(WasmtimePlugin { store: sandbox.store, plugin, budget })
    }
}

impl PluginBackend for WasmtimePlugin {
    fn info(&mut self) -> Result<sdk::PluginInfo> {
        // `info` is part of starting the plugin, so it runs on the load allowance.
        self.plugin
            .lexical_editor_plugin()
            .call_info(&mut self.store)
            .map(info_from)
            .map_err(|e| PluginError::Load(format!("info() failed: {e:#}")))
    }

    fn handle_command(&mut self, cmd: &sdk::Command, ctx: &sdk::CommandContext) -> Result<sdk::Outcome> {
        arm(&mut self.store, self.budget)?;
        self.plugin
            .lexical_editor_plugin()
            .call_handle_command(&mut self.store, &command_to(cmd), &command_context_to(ctx))
            .map(outcome_from)
            .map_err(call_error)
    }

    fn transform_text(&mut self, ctx: &sdk::TextContext) -> Result<Vec<sdk::Op>> {
        arm(&mut self.store, self.budget)?;
        self.plugin
            .lexical_editor_plugin()
            .call_transform_text(&mut self.store, &text_context_to(ctx))
            .map(ops_from)
            .map_err(call_error)
    }
}

/// A `lexical:editor/document-plugin` component in a wasmtime store.
pub struct WasmtimeDocument {
    store: Store<HostState>,
    plugin: LexicalDocumentPlugin,
    budget: Budget,
}

impl WasmtimeDocument {
    pub fn new(bytes: &[u8], budget: Budget) -> Result<WasmtimeDocument> {
        let mut sandbox = prepare(bytes, budget)?;
        let plugin = LexicalDocumentPlugin::instantiate(&mut sandbox.store, &sandbox.component, &sandbox.linker)
            .map_err(load_error)?;
        Ok(WasmtimeDocument { store: sandbox.store, plugin, budget })
    }
}

impl DocumentBackend for WasmtimeDocument {
    fn run(
        &mut self,
        state: &str,
        selection: Option<DocumentSelection>,
        command: &str,
        payload: &str,
    ) -> Result<DocumentOutcome> {
        arm(&mut self.store, self.budget)?;
        let selection = selection.as_ref().map(selection_to);
        self.plugin
            .lexical_editor_document_plugin()
            .call_run(&mut self.store, state, selection, command, payload)
            .map(document_outcome_from)
            .map_err(call_error)
    }
}
