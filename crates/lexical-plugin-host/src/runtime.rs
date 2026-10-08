//! Loading a plugin component and wiring it into an [`Editor`].

use crate::bindings::lexical::editor::types as w;
use crate::bindings::LexicalPlugin;
use crate::convert::command_to_wit;
use crate::error::{PluginError, Result};
use crate::ops::{apply_in_transform, apply_to_editor, command_context, text_context};
use crate::sandbox::{arm, load_error, prepare, record, Budget, HostState};
use lexical_core::{Editor, ListenerId, NodeType, Plugin};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use wasmtime::Store;

/// Deepest chain of plugin → command → plugin → command… before it is cut off.
const MAX_REENTRY: u32 = 8;

struct Instance {
    store: Store<HostState>,
    plugin: LexicalPlugin,
    budget: Budget,
}

impl Instance {
    fn arm(&mut self) -> Result<()> {
        arm(&mut self.store, self.budget)
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
        let mut sandbox = prepare(bytes, budget)?;
        let plugin = LexicalPlugin::instantiate(&mut sandbox.store, &sandbox.component, &sandbox.linker)
            .map_err(load_error)?;
        let mut store = sandbox.store;
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
            // Read-only is enforced here, not left to the plugin: a read-only editor allows
            // selection changes only, so plugins are not consulted and cannot mutate it.
            if !ed.is_editable() {
                return false;
            }
            if depth.get() >= MAX_REENTRY {
                record(&errors, PluginError::Call("plugin commands nest too deeply".into()));
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
                    record(&errors, e);
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
                        record(&errors, e);
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
