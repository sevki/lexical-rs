//! The [`Editor`]: owns the committed state and runs the update pipeline
//! (mutate a pending copy -> transforms -> normalize -> commit -> notify).
//!
//! * `update` – the transactional pipeline and listener notification
//! * `undo` – stepping through history
//! * `commands` – the [`Command`] vocabulary and its dispatch

mod commands;
mod undo;
mod update;

pub use commands::Command;

use crate::error::Result;
use crate::history::{ChangeKind, History};
use crate::limits::Limits;
use crate::node::*;
use crate::state::EditorState;
use std::collections::{BTreeSet, HashMap};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tag {
    /// The update came from undo/redo; never recorded in history.
    Historic,
    /// Fold this update into the previous undo step.
    HistoryMerge,
    /// Always start a new undo step.
    HistoryPush,
    /// Only the selection changed.
    SelectionOnly,
    /// The document was replaced from outside the local edit stream (a remote peer's
    /// change, or an undo performed by an external history). Never recorded in the
    /// built-in history.
    Remote,
    Kind(ChangeKind),
}

/// What a commit hook sees: the transition, before listeners are notified.
pub struct CommitInfo<'a> {
    pub prev: &'a EditorState,
    pub state: &'a EditorState,
    pub tags: &'a [Tag],
    /// True when nodes (not just the selection) changed.
    pub content_changed: bool,
}

pub struct UpdateEvent<'a> {
    pub prev: &'a EditorState,
    pub state: &'a EditorState,
    pub dirty: &'a BTreeSet<NodeKey>,
    pub created: &'a [NodeKey],
    pub destroyed: &'a [NodeKey],
    pub tags: &'a [Tag],
    /// History availability after this update (listeners cannot borrow the editor).
    pub can_undo: bool,
    pub can_redo: bool,
}

impl UpdateEvent<'_> {
    pub fn has_tag(&self, t: Tag) -> bool {
        self.tags.contains(&t)
    }
    /// True when nodes (not just the selection) changed.
    pub fn content_changed(&self) -> bool {
        !self.has_tag(Tag::SelectionOnly) && !self.dirty.is_empty()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ListenerId(u64);

type UpdateListener = Box<dyn FnMut(&UpdateEvent)>;
type CommitHook = Box<dyn FnMut(&CommitInfo)>;
type CommandHandler = Box<dyn FnMut(&mut Editor, &Command) -> bool>;
type Transform = Box<dyn Fn(&mut EditorState, NodeKey) -> Result<()>>;
/// Reports `(can_undo, can_redo)` for an external history (e.g. a collaborative one).
type HistoryProvider = Box<dyn Fn() -> (bool, bool)>;

/// Extension point mirroring Lexical iOS's `Plugin` protocol.
pub trait Plugin {
    fn set_up(&mut self, editor: &mut Editor);
    fn tear_down(&mut self, _editor: &mut Editor) {}
}

pub struct Editor {
    state: EditorState,
    update_listeners: Vec<(ListenerId, UpdateListener)>,
    commit_hooks: Vec<(ListenerId, CommitHook)>,
    command_handlers: Vec<(i32, ListenerId, CommandHandler)>,
    transforms: HashMap<NodeType, Vec<(ListenerId, Transform)>>,
    history: History,
    history_provider: Option<HistoryProvider>,
    editable: bool,
    next_id: u64,
    plugins: Vec<Box<dyn Plugin>>,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

impl Editor {
    pub fn new() -> Editor {
        Editor::with_state(EditorState::new())
    }

    pub fn with_state(state: EditorState) -> Editor {
        Editor {
            state,
            update_listeners: vec![],
            commit_hooks: vec![],
            command_handlers: vec![],
            transforms: HashMap::new(),
            history: History::default(),
            history_provider: None,
            editable: true,
            next_id: 1,
            plugins: vec![],
        }
    }

    pub fn state(&self) -> &EditorState {
        &self.state
    }

    pub fn read<R>(&self, f: impl FnOnce(&EditorState) -> R) -> R {
        f(&self.state)
    }

    pub fn is_editable(&self) -> bool {
        self.editable
    }

    pub fn set_editable(&mut self, e: bool) {
        self.editable = e;
    }

    /// Optional caps on indentation; unlimited by default and kept across `set_state`.
    pub fn set_limits(&mut self, limits: Limits) {
        self.state.limits = limits;
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn history_mut(&mut self) -> &mut History {
        &mut self.history
    }

    fn fresh_id(&mut self) -> ListenerId {
        self.next_id += 1;
        ListenerId(self.next_id)
    }

    pub fn register_update_listener(&mut self, f: impl FnMut(&UpdateEvent) + 'static) -> ListenerId {
        let id = self.fresh_id();
        self.update_listeners.push((id, Box::new(f)));
        id
    }

    /// Runs on every commit after the new state is in place and *before* listeners are
    /// notified, so a hook can bring external state (a CRDT, a journal) up to date first.
    pub fn register_commit_hook(&mut self, f: impl FnMut(&CommitInfo) + 'static) -> ListenerId {
        let id = self.fresh_id();
        self.commit_hooks.push((id, Box::new(f)));
        id
    }

    /// Route undo/redo availability to an external history. While set, the built-in
    /// history records nothing (a snapshot undo would also revert other peers' edits);
    /// the host intercepts `Command::Undo`/`Redo` with a command handler.
    pub fn set_history_provider(&mut self, provider: Option<impl Fn() -> (bool, bool) + 'static>) {
        self.history_provider = provider.map(|p| Box::new(p) as HistoryProvider);
        if self.history_provider.is_some() {
            self.history.clear();
        }
    }

    /// Higher priority handlers run first; the first to return `true` consumes the command.
    pub fn register_command(
        &mut self,
        priority: i32,
        f: impl FnMut(&mut Editor, &Command) -> bool + 'static,
    ) -> ListenerId {
        let id = self.fresh_id();
        self.command_handlers.push((priority, id, Box::new(f)));
        self.command_handlers.sort_by_key(|(p, _, _)| -*p);
        id
    }

    pub fn register_node_transform(
        &mut self,
        ty: NodeType,
        f: impl Fn(&mut EditorState, NodeKey) -> Result<()> + 'static,
    ) -> ListenerId {
        let id = self.fresh_id();
        self.transforms.entry(ty).or_default().push((id, Box::new(f)));
        id
    }

    pub fn unregister(&mut self, id: ListenerId) {
        self.update_listeners.retain(|(i, _)| *i != id);
        self.commit_hooks.retain(|(i, _)| *i != id);
        self.command_handlers.retain(|(_, i, _)| *i != id);
        for v in self.transforms.values_mut() {
            v.retain(|(i, _)| *i != id);
        }
    }

    pub fn add_plugin(&mut self, mut plugin: Box<dyn Plugin>) {
        plugin.set_up(self);
        self.plugins.push(plugin);
    }
}
