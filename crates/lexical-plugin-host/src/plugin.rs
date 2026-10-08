//! Wiring a plugin backend into an [`Editor`].

use crate::backend::PluginBackend;
use crate::convert::command_to_wit;
use crate::error::{record, PluginError, Result};
use crate::ops::{apply_in_transform, apply_to_editor, command_context, text_context};
use lexical_core::{Editor, ListenerId, NodeType, Plugin};
use lexical_plugin::PluginInfo;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Deepest chain of plugin → command → plugin → command… before it is cut off.
const MAX_REENTRY: u32 = 8;

/// A plugin component on some runtime, ready to be added to an editor with
/// [`Editor::add_plugin`].
///
/// The plugin sees the commands in the WIT `command` variant and text-node transforms, and
/// answers with operations the host applies. A read-only editor never consults it, and a
/// call that fails (a trap, an exhausted budget) leaves the editor untouched and falls
/// through to the next handler.
pub struct ComponentPlugin<B> {
    backend: Rc<RefCell<B>>,
    info: PluginInfo,
    errors: Rc<RefCell<Vec<PluginError>>>,
    registered: Vec<ListenerId>,
}

impl<B: PluginBackend + 'static> ComponentPlugin<B> {
    /// Ask the backend for the plugin's `info` and keep it.
    pub fn new(mut backend: B) -> Result<ComponentPlugin<B>> {
        let info = backend.info()?;
        Ok(ComponentPlugin { backend: Rc::new(RefCell::new(backend)), info, errors: Rc::default(), registered: vec![] })
    }

    pub fn name(&self) -> &str {
        &self.info.name
    }

    /// Failures of calls into the plugin, newest last, at most 64 of them.
    pub fn errors(&self) -> Rc<RefCell<Vec<PluginError>>> {
        self.errors.clone()
    }
}

impl<B: PluginBackend + 'static> Plugin for ComponentPlugin<B> {
    fn set_up(&mut self, editor: &mut Editor) {
        let depth = Rc::new(Cell::new(0u32));

        let (backend, errors) = (self.backend.clone(), self.errors.clone());
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
            let outcome = backend.borrow_mut().handle_command(&wit_cmd, &ctx);
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
            let (backend, errors) = (self.backend.clone(), self.errors.clone());
            let id = editor.register_node_transform(NodeType::Text, move |state, key| {
                let Some(ctx) = text_context(state, key) else { return Ok(()) };
                let ops = backend.borrow_mut().transform_text(&ctx);
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
