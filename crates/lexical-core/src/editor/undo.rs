//! Stepping through history.

use super::{Editor, Tag};
use crate::state::EditorState;

impl Editor {
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
        snap.limits = self.state.limits;
        snap.dirty = snap.nodes.keys().copied().collect();
        self.history.break_coalescing();
        self.commit(snap, &[Tag::Historic]);
    }
}
