//! The update pipeline: run a closure against a pending copy of the state, settle
//! node transforms, normalize, commit, then notify listeners.

use super::{CommitInfo, Editor, Tag, UpdateEvent};
use crate::error::{Error, Result};
use crate::history::ChangeKind;
use crate::node::*;
use crate::state::EditorState;
use std::collections::BTreeSet;

const MAX_TRANSFORM_PASSES: usize = 100;

impl Editor {
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
                let Some(ts) = self.transforms.get(&n.node_type()) else {
                    continue;
                };
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

    pub(super) fn commit(&mut self, pending: EditorState, tags: &[Tag]) {
        let prev = std::mem::replace(&mut self.state, pending);
        let dirty: BTreeSet<NodeKey> = self.state.dirty.clone();
        let content = !tags.contains(&Tag::SelectionOnly)
            && dirty
                .iter()
                .any(|k| self.state.contains(*k) || prev.contains(*k));
        let created: Vec<_> = dirty
            .iter()
            .copied()
            .filter(|k| self.state.contains(*k) && !prev.contains(*k))
            .collect();
        let destroyed: Vec<_> = dirty
            .iter()
            .copied()
            .filter(|k| prev.contains(*k) && !self.state.contains(*k))
            .collect();
        self.state.dirty.clear();

        let mut hooks = std::mem::take(&mut self.commit_hooks);
        let info = CommitInfo {
            prev: &prev,
            state: &self.state,
            tags,
            content_changed: content,
        };
        for (_, h) in hooks.iter_mut() {
            h(&info);
        }
        hooks.append(&mut self.commit_hooks);
        self.commit_hooks = hooks;

        let recorded = !tags.contains(&Tag::Historic) && !tags.contains(&Tag::Remote);
        if content && recorded && self.history_provider.is_none() {
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
            can_undo: self.can_undo(),
            can_redo: self.can_redo(),
        };
        for (_, l) in listeners.iter_mut() {
            l(&ev);
        }
        listeners.append(&mut self.update_listeners);
        self.update_listeners = listeners;
    }

    /// Replace the whole document (e.g. after loading JSON). Clears history; the
    /// editor's [`Limits`](crate::Limits) carry over.
    pub fn set_state(&mut self, state: EditorState) {
        let mut state = state;
        state.limits = self.state.limits;
        if state.selection.is_none()
            && let Some(&b) = state.line_blocks().first()
        {
            state.selection = Some(crate::Selection::collapsed(crate::Point::element(b, 0)));
        }
        state.dirty = state.nodes.keys().copied().collect();
        self.history.clear();
        self.commit(state, &[Tag::Historic]);
    }

    /// Replace the document with one produced outside the local edit stream (a remote
    /// peer's change, an external undo). Not recorded in history, bypasses transforms,
    /// and is tagged [`Tag::Remote`] so hooks and listeners can tell it apart. The
    /// editor's limits carry over; the caller supplies the selection.
    pub fn apply_remote(&mut self, mut state: EditorState) {
        state.limits = self.state.limits;
        state.validate_selection();
        state.dirty = state.nodes.keys().copied().collect();
        self.commit(state, &[Tag::Remote]);
    }

    /// Update only the selection (e.g. from the view), without touching history.
    pub fn set_selection(&mut self, anchor: crate::Point, focus: crate::Point) {
        let _ = self.update_tagged(&[Tag::SelectionOnly], |s| {
            s.set_selection_points(anchor, focus);
            Ok(())
        });
        self.history.break_coalescing();
    }
}
