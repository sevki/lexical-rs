//! Loading a plugin component and wiring it into an [`Editor`].

use crate::bindings::lexical::editor::types as w;
use crate::bindings::LexicalPlugin;
use crate::convert::command_to_wit;
use crate::error::{PluginError, Result};
use crate::ops::{apply_in_transform, apply_to_editor, command_context, text_context};
use lexical_core::{Editor, ListenerId, NodeType, Plugin};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

/// What one call into a plugin may consume before it is stopped.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    /// Wasm instructions (roughly) per call.
    pub fuel: u64,
    /// Linear memory the plugin may grow to.
    pub memory_bytes: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Budget { fuel: 50_000_000, memory_bytes: 64 << 20 }
    }
}

/// Fuel for instantiating a plugin and asking it for its `info`.
const LOAD_FUEL: u64 = 500_000_000;

/// Deepest chain of plugin → command → plugin → command… before it is cut off.
const MAX_REENTRY: u32 = 8;

struct HostState {
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView { ctx: &mut self.wasi, table: &mut self.table }
    }
}

struct Instance {
    store: Store<HostState>,
    plugin: LexicalPlugin,
    budget: Budget,
}

impl Instance {
    fn arm(&mut self) -> Result<()> {
        self.store.set_fuel(self.budget.fuel).map_err(|e| PluginError::Call(e.to_string()))
    }

    fn handle_command(&mut self, cmd: w::Command, ctx: w::CommandContext) -> Result<w::Outcome> {
        self.arm()?;
        self.plugin
            .lexical_editor_plugin()
            .call_handle_command(&mut self.store, &cmd, &ctx)
            .map_err(|e| PluginError::Call(format!("{e:#}")))
    }

    fn transform_text(&mut self, ctx: w::TextContext) -> Result<Vec<w::Op>> {
        self.arm()?;
        self.plugin
            .lexical_editor_plugin()
            .call_transform_text(&mut self.store, &ctx)
            .map_err(|e| PluginError::Call(format!("{e:#}")))
    }
}

/// A plugin component, ready to be added to an editor with [`Editor::add_plugin`].
pub struct WasmPlugin {
    instance: Rc<RefCell<Instance>>,
    info: w::PluginInfo,
    errors: Rc<RefCell<Vec<PluginError>>>,
    registered: Vec<ListenerId>,
}

impl WasmPlugin {
    /// Load a component from its binary form (`.wasm`) with the default [`Budget`].
    pub fn load(bytes: &[u8]) -> Result<WasmPlugin> {
        Self::load_with(bytes, Budget::default())
    }

    pub fn load_with(bytes: &[u8], budget: Budget) -> Result<WasmPlugin> {
        let load = |e: wasmtime::Error| PluginError::Load(format!("{e:#}"));
        let mut config = Config::new();
        config.consume_fuel(true);
        let engine = Engine::new(&config).map_err(load)?;
        let component = Component::new(&engine, bytes).map_err(load)?;

        // WASI is linked because Rust's standard library imports it; the guest is given an
        // empty context: no files, no environment, no network, no inherited stdio.
        let mut linker = Linker::<HostState>::new(&engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker).map_err(load)?;

        let state = HostState {
            wasi: WasiCtx::builder().build(),
            table: ResourceTable::new(),
            // A component is several core instances (the guest, adapters, shims).
            limits: StoreLimitsBuilder::new().memory_size(budget.memory_bytes).instances(32).build(),
        };
        let mut store = Store::new(&engine, state);
        store.limiter(|s| &mut s.limits);
        // Starting the plugin gets a fixed allowance; the budget applies to each call.
        store.set_fuel(LOAD_FUEL).map_err(load)?;
        let plugin = LexicalPlugin::instantiate(&mut store, &component, &linker).map_err(load)?;
        let info = plugin
            .lexical_editor_plugin()
            .call_info(&mut store)
            .map_err(|e| PluginError::Load(format!("info() failed: {e:#}")))?;
        Ok(WasmPlugin {
            instance: Rc::new(RefCell::new(Instance { store, plugin, budget })),
            info,
            errors: Rc::default(),
            registered: vec![],
        })
    }

    pub fn name(&self) -> &str {
        &self.info.name
    }

    /// Failures of calls into the plugin (traps, running out of fuel…). A failing call
    /// leaves the editor untouched; the command falls through to the next handler.
    pub fn errors(&self) -> Rc<RefCell<Vec<PluginError>>> {
        self.errors.clone()
    }
}

impl Plugin for WasmPlugin {
    fn set_up(&mut self, editor: &mut Editor) {
        let depth = Rc::new(Cell::new(0u32));

        let (instance, errors) = (self.instance.clone(), self.errors.clone());
        let id = editor.register_command(self.info.priority, move |ed, cmd| {
            let Some(wit_cmd) = command_to_wit(cmd) else { return false };
            if depth.get() >= MAX_REENTRY {
                errors.borrow_mut().push(PluginError::Call("plugin commands nest too deeply".into()));
                return false;
            }
            let ctx = command_context(ed.state(), ed.is_editable());
            // The borrow ends before the ops run: they may dispatch commands that come
            // straight back to this plugin.
            let outcome = instance.borrow_mut().handle_command(wit_cmd, ctx);
            match outcome {
                Ok(outcome) => {
                    depth.set(depth.get() + 1);
                    apply_to_editor(ed, outcome.ops);
                    depth.set(depth.get() - 1);
                    outcome.handled
                }
                Err(e) => {
                    errors.borrow_mut().push(e);
                    false
                }
            }
        });
        self.registered.push(id);

        if self.info.transforms_text {
            let (instance, errors) = (self.instance.clone(), self.errors.clone());
            let id = editor.register_node_transform(NodeType::Text, move |state, key| {
                let Some(ctx) = text_context(state, key) else { return Ok(()) };
                let ops = instance.borrow_mut().transform_text(ctx);
                match ops {
                    Ok(ops) => apply_in_transform(state, key, &ops),
                    Err(e) => {
                        errors.borrow_mut().push(e);
                        Ok(())
                    }
                }
            });
            self.registered.push(id);
        }
    }

    fn tear_down(&mut self, editor: &mut Editor) {
        for id in self.registered.drain(..) {
            editor.unregister(id);
        }
    }
}
