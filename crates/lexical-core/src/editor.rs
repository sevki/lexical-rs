//! The [`Editor`]: owns the committed state and runs the update pipeline
//! (mutate a pending copy -> transforms -> normalize -> commit -> notify).

use crate::blocks::BlockType;
use crate::error::{Error, Result};
use crate::format::{Align, TextFormat};
use crate::history::{ChangeKind, History};
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
    Kind(ChangeKind),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    InsertText(String),
    Paste(String),
    InsertParagraph,
    InsertLineBreak,
    DeleteCharacter { backward: bool },
    DeleteWord { backward: bool },
    DeleteLine { backward: bool },
    FormatText(TextFormat),
    FormatAlign(Align),
    SetBlockType(BlockType),
    ToggleList(ListType),
    ToggleCheck,
    ToggleLink(Option<String>),
    Indent,
    Outdent,
    SelectAll,
    Undo,
    Redo,
    Custom(String),
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
type CommandHandler = Box<dyn FnMut(&mut Editor, &Command) -> bool>;
type Transform = Box<dyn Fn(&mut EditorState, NodeKey) -> Result<()>>;

/// Extension point mirroring Lexical iOS's `Plugin` protocol.
pub trait Plugin {
    fn set_up(&mut self, editor: &mut Editor);
    fn tear_down(&mut self, _editor: &mut Editor) {}
}

pub struct Editor {
    state: EditorState,
    update_listeners: Vec<(ListenerId, UpdateListener)>,
    command_handlers: Vec<(i32, ListenerId, CommandHandler)>,
    transforms: HashMap<NodeType, Vec<(ListenerId, Transform)>>,
    history: History,
    editable: bool,
    next_id: u64,
    plugins: Vec<Box<dyn Plugin>>,
}

const MAX_TRANSFORM_PASSES: usize = 100;

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
            command_handlers: vec![],
            transforms: HashMap::new(),
            history: History::default(),
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

    // ------------------------------------------------------------ registration

    pub fn register_update_listener(&mut self, f: impl FnMut(&UpdateEvent) + 'static) -> ListenerId {
        let id = self.fresh_id();
        self.update_listeners.push((id, Box::new(f)));
        id
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
        self.command_handlers.retain(|(_, i, _)| *i != id);
        for v in self.transforms.values_mut() {
            v.retain(|(i, _)| *i != id);
        }
    }

    pub fn add_plugin(&mut self, mut plugin: Box<dyn Plugin>) {
        plugin.set_up(self);
        self.plugins.push(plugin);
    }

    // ----------------------------------------------------------------- updates

    pub fn update(&mut self, f: impl FnOnce(&mut EditorState) -> Result<()>) -> Result<()> {
        self.update_tagged(&[], f)
    }

    /// Run `f` against a pending copy of the state. On `Err` nothing is committed.
    pub fn update_tagged(
        &mut self,
        tags: &[Tag],
        f: impl FnOnce(&mut EditorState) -> Result<()>,
    ) -> Result<()> {
        let mut pending = self.state.clone();
        pending.dirty.clear();
        f(&mut pending)?;
        self.run_transforms(&mut pending)?;
        pending.normalize_dirty();
        pending.validate_selection();
        self.commit(pending, tags);
        Ok(())
    }

    fn run_transforms(&self, s: &mut EditorState) -> Result<()> {
        if self.transforms.is_empty() {
            return Ok(());
        }
        // Transforms must be idempotent: a node a transform re-dirties is re-queued.
        let mut queue: BTreeSet<NodeKey> = std::mem::take(&mut s.dirty);
        let mut all = queue.clone();
        for _ in 0..MAX_TRANSFORM_PASSES {
            if queue.is_empty() {
                s.dirty = all;
                return Ok(());
            }
            for k in std::mem::take(&mut queue) {
                let Some(n) = s.get(k) else { continue };
                let Some(ts) = self.transforms.get(&n.node_type()) else { continue };
                for (_, t) in ts {
                    if !s.contains(k) {
                        break;
                    }
                    t(s, k)?;
                }
            }
            queue = std::mem::take(&mut s.dirty);
            all.extend(queue.iter().copied());
        }
        Err(Error::TransformLoop)
    }

    fn commit(&mut self, pending: EditorState, tags: &[Tag]) {
        let prev = std::mem::replace(&mut self.state, pending);
        let dirty: BTreeSet<NodeKey> = self.state.dirty.clone();
        let content = !tags.contains(&Tag::SelectionOnly)
            && dirty.iter().any(|k| self.state.contains(*k) || prev.contains(*k));
        let created: Vec<_> =
            dirty.iter().copied().filter(|k| self.state.contains(*k) && !prev.contains(*k)).collect();
        let destroyed: Vec<_> =
            dirty.iter().copied().filter(|k| prev.contains(*k) && !self.state.contains(*k)).collect();
        self.state.dirty.clear();

        if content && !tags.contains(&Tag::Historic) {
            let kind = tags
                .iter()
                .find_map(|t| if let Tag::Kind(k) = t { Some(*k) } else { None })
                .unwrap_or(ChangeKind::Other);
            self.history.record(
                prev.clone(),
                kind,
                tags.contains(&Tag::HistoryMerge),
                tags.contains(&Tag::HistoryPush),
            );
        }
        let mut tags_v = tags.to_vec();
        if !content && !tags_v.contains(&Tag::SelectionOnly) {
            tags_v.push(Tag::SelectionOnly);
        }
        let mut listeners = std::mem::take(&mut self.update_listeners);
        let ev = UpdateEvent {
            prev: &prev,
            state: &self.state,
            dirty: &dirty,
            created: &created,
            destroyed: &destroyed,
            tags: &tags_v,
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
        };
        for (_, l) in listeners.iter_mut() {
            l(&ev);
        }
        listeners.append(&mut self.update_listeners);
        self.update_listeners = listeners;
    }

    /// Replace the whole document (e.g. after loading JSON). Clears history.
    pub fn set_state(&mut self, state: EditorState) {
        let mut state = state;
        if state.selection.is_none()
            && let Some(&b) = state.line_blocks().first() {
                state.selection = Some(crate::Selection::collapsed(crate::Point::element(b, 0)));
            }
        state.dirty = state.nodes.keys().copied().collect();
        self.history.clear();
        self.commit(state, &[Tag::Historic]);
    }

    /// Update only the selection (e.g. from the view), without touching history.
    pub fn set_selection(&mut self, anchor: crate::Point, focus: crate::Point) {
        let _ = self.update_tagged(&[Tag::SelectionOnly], |s| {
            s.set_selection_points(anchor, focus);
            Ok(())
        });
        self.history.break_coalescing();
    }

    // ------------------------------------------------------------ undo / redo

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn undo(&mut self) -> bool {
        let Some(snap) = self.history.undo.pop() else { return false };
        self.history.redo.push(self.state.clone());
        self.restore(snap);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(snap) = self.history.redo.pop() else { return false };
        self.history.undo.push(self.state.clone());
        self.restore(snap);
        true
    }

    fn restore(&mut self, mut snap: EditorState) {
        snap.dirty = snap.nodes.keys().copied().collect();
        self.history.break_coalescing();
        self.commit(snap, &[Tag::Historic]);
    }

    // --------------------------------------------------------------- commands

    /// Dispatch a command: registered handlers first, then the built-in behaviour.
    /// Returns whether anything handled it.
    pub fn dispatch(&mut self, cmd: Command) -> bool {
        let mut handlers = std::mem::take(&mut self.command_handlers);
        let mut handled = false;
        for (_, _, h) in handlers.iter_mut() {
            if h(self, &cmd) {
                handled = true;
                break;
            }
        }
        handlers.append(&mut self.command_handlers);
        handlers.sort_by_key(|(p, _, _)| -*p);
        self.command_handlers = handlers;
        handled || self.handle_default(&cmd)
    }

    fn handle_default(&mut self, cmd: &Command) -> bool {
        use Command::*;
        // A read-only editor allows selection changes only; history would replace the
        // document, so it is gated like every other edit.
        if !self.editable && !matches!(cmd, SelectAll) {
            return false;
        }
        match cmd {
            Undo => return self.undo(),
            Redo => return self.redo(),
            _ => {}
        }
        let kind = match cmd {
            InsertText(t) if !t.contains('\n') => ChangeKind::Typing,
            DeleteCharacter { backward: true } => ChangeKind::DeleteBackward,
            DeleteCharacter { backward: false } => ChangeKind::DeleteForward,
            _ => ChangeKind::Other,
        };
        let tags = [Tag::Kind(kind)];
        let r = self.update_tagged(&tags, |s| match cmd {
            InsertText(t) | Paste(t) => {
                if matches!(cmd, Paste(_)) {
                    s.insert_raw_text(t)
                } else {
                    s.insert_text(t)
                }
            }
            InsertParagraph => s.insert_paragraph(),
            InsertLineBreak => s.insert_line_break(),
            DeleteCharacter { backward } => s.delete_character(*backward),
            DeleteWord { backward } => s.delete_word(*backward),
            DeleteLine { backward } => s.delete_line(*backward),
            FormatText(f) => s.format_text(*f),
            FormatAlign(a) => s.set_align(*a),
            SetBlockType(b) => s.set_block_type(*b),
            ToggleList(t) => s.toggle_list(*t),
            ToggleCheck => {
                for b in s.selected_blocks() {
                    s.toggle_check(b);
                }
                Ok(())
            }
            ToggleLink(u) => s.toggle_link(u.as_deref()),
            Indent => s.indent_blocks(),
            Outdent => s.outdent_blocks(),
            SelectAll => s.select_all(),
            Undo | Redo | Custom(_) => Err(Error::Invalid("unhandled".into())),
        });
        r.is_ok() && !matches!(cmd, Custom(_))
    }
}

impl EditorState {
    pub fn select_all(&mut self) -> Result<()> {
        let blocks = self.line_blocks();
        let (Some(&f), Some(&l)) = (blocks.first(), blocks.last()) else { return Ok(()) };
        let end = self.node(l).children.len();
        self.set_selection_points(crate::Point::element(f, 0), crate::Point::element(l, end));
        Ok(())
    }
}
