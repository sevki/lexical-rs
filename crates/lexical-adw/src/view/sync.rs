//! Keeping the `GtkTextBuffer` and the engine in step, in both directions.

use super::Inner;
use crate::reconciler;
use adw::gtk::{self, prelude::*};
use lexical_core::{BlockStyle, EditorState, Layout, ListType, Tag, UpdateEvent};

impl Inner {
    /// Engine -> widget: reconcile the buffer, restore the selection, refresh the toolbar.
    pub(super) fn on_update(&self, ev: &UpdateEvent) {
        let layout = if ev.content_changed() || ev.has_tag(Tag::Historic) {
            let l = Layout::build(ev.state);
            self.syncing.set(true);
            reconciler::apply(&self.buffer, &self.tags, &l);
            self.syncing.set(false);
            *self.layout.borrow_mut() = l.clone();
            l
        } else {
            self.layout.borrow().clone()
        };
        self.syncing.set(true);
        self.apply_selection(ev.state, &layout);
        self.syncing.set(false);
        self.toolbar.sync(ev.state, ev.can_undo, ev.can_redo);
        let listeners: Vec<_> = self.listeners.borrow().clone();
        for l in listeners {
            l(ev.state);
        }
    }

    pub(super) fn apply_selection(&self, state: &EditorState, layout: &Layout) {
        let Some(sel) = &state.selection else { return };
        let (a, f) = (layout.offset_of(state, &sel.anchor), layout.offset_of(state, &sel.focus));
        let cur_ins = self.buffer.iter_at_mark(&self.buffer.get_insert()).offset() as usize;
        let cur_bound = self.buffer.iter_at_mark(&self.buffer.selection_bound()).offset() as usize;
        if (cur_ins, cur_bound) == (f, a) {
            return;
        }
        let (fi, ai) = (self.buffer.iter_at_offset(f as i32), self.buffer.iter_at_offset(a as i32));
        self.buffer.select_range(&fi, &ai);
    }

    /// Widget -> engine: a native cursor/selection move.
    pub(super) fn selection_from_buffer(&self) {
        if self.syncing.get() {
            return;
        }
        let ins = self.buffer.iter_at_mark(&self.buffer.get_insert()).offset() as usize;
        let bound = self.buffer.iter_at_mark(&self.buffer.selection_bound()).offset() as usize;
        let Ok(mut ed) = self.editor.try_borrow_mut() else { return };
        let (a, f) = {
            let l = self.layout.borrow();
            (l.point_at(ed.state(), bound), l.point_at(ed.state(), ins))
        };
        if ed.state().selection.as_ref().is_some_and(|s| s.anchor == a && s.focus == f) {
            return;
        }
        ed.set_selection(a, f);
    }

    /// A click on a checklist marker toggles the item.
    pub(super) fn click_marker(&self, x: f64, y: f64) {
        let (bx, by) =
            self.text_view.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
        let Some(iter) = self.text_view.iter_at_location(bx, by) else { return };
        let off = iter.offset() as usize;
        let key = {
            let layout = self.layout.borrow();
            match layout.line_at(off) {
                Some(l)
                    if off < l.content_start
                        && matches!(l.style, BlockStyle::ListItem { list_type: ListType::Check, .. }) =>
                {
                    l.key
                }
                _ => return,
            }
        };
        if let Ok(mut ed) = self.editor.try_borrow_mut() {
            let _ = ed.update(|s| {
                s.toggle_check(key);
                Ok(())
            });
        }
    }
}
