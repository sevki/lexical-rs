//! Undo/redo stacks of whole-state snapshots, with typing coalescing.

use crate::state::EditorState;
use std::time::{Duration, Instant};

/// Kind of change, used to decide whether consecutive updates share an undo step.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeKind {
    Typing,
    DeleteBackward,
    DeleteForward,
    Other,
}

#[derive(Debug)]
pub struct History {
    pub(crate) undo: Vec<EditorState>,
    pub(crate) redo: Vec<EditorState>,
    last_kind: ChangeKind,
    last_time: Option<Instant>,
    pub merge_window: Duration,
    pub limit: usize,
}

impl Default for History {
    fn default() -> Self {
        History {
            undo: vec![],
            redo: vec![],
            last_kind: ChangeKind::Other,
            last_time: None,
            merge_window: Duration::from_millis(1000),
            limit: 200,
        }
    }
}

impl History {
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.last_kind = ChangeKind::Other;
        self.last_time = None;
    }

    /// Record that `prev` was replaced by a content change.
    pub(crate) fn record(&mut self, prev: EditorState, kind: ChangeKind, force_merge: bool, force_push: bool) {
        let now = Instant::now();
        let within = self.last_time.is_some_and(|t| now.duration_since(t) < self.merge_window);
        let merge = !force_push
            && !self.undo.is_empty()
            && (force_merge || (kind != ChangeKind::Other && kind == self.last_kind && within));
        if !merge {
            self.undo.push(prev);
            if self.undo.len() > self.limit {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last_kind = kind;
        self.last_time = Some(now);
    }

    pub(crate) fn break_coalescing(&mut self) {
        self.last_kind = ChangeKind::Other;
    }
}
