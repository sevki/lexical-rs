//! Keyboard shortcuts, the input method (typing / IME commits) and the clipboard.

use super::LexicalView;
use adw::gtk::{self, gdk, gio, glib, prelude::*};
use lexical_core::{Command, TextFormat};
use std::rc::Rc;

impl LexicalView {
    /// Key binding handler shared by the key controller and tests.
    #[doc(hidden)]
    pub fn handle_key(&self, key: gdk::Key, state: gdk::ModifierType) -> bool {
        let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
        let lower = key.to_lower();
        let is = |k: gdk::Key| key == k || lower == k;
        let cmd = if is(gdk::Key::Return) || is(gdk::Key::KP_Enter) {
            if shift { Command::InsertLineBreak } else { Command::InsertParagraph }
        } else if is(gdk::Key::BackSpace) {
            Command::DeleteCharacter { backward: true }.or_word(ctrl)
        } else if is(gdk::Key::Delete) || is(gdk::Key::KP_Delete) {
            Command::DeleteCharacter { backward: false }.or_word(ctrl)
        } else if is(gdk::Key::ISO_Left_Tab) {
            Command::Outdent
        } else if is(gdk::Key::Tab) && !ctrl {
            if shift { Command::Outdent } else { Command::Indent }
        } else if ctrl {
            if is(gdk::Key::b) {
                Command::FormatText(TextFormat::BOLD)
            } else if is(gdk::Key::i) {
                Command::FormatText(TextFormat::ITALIC)
            } else if is(gdk::Key::u) {
                Command::FormatText(TextFormat::UNDERLINE)
            } else if is(gdk::Key::z) {
                if shift { Command::Redo } else { Command::Undo }
            } else if is(gdk::Key::y) {
                Command::Redo
            } else if is(gdk::Key::a) {
                Command::SelectAll
            } else if is(gdk::Key::c) {
                self.copy();
                return true;
            } else if is(gdk::Key::x) {
                if self.copy() {
                    self.dispatch(Command::DeleteCharacter { backward: true });
                }
                return true;
            } else if is(gdk::Key::v) {
                self.paste();
                return true;
            } else {
                return false;
            }
        } else {
            return false;
        };
        self.dispatch(cmd);
        true
    }

    fn copy(&self) -> bool {
        let text = self.inner.editor.borrow().state().selected_text();
        if text.is_empty() {
            return false;
        }
        self.inner.text_view.clipboard().set_text(&text);
        true
    }

    fn paste(&self) {
        let this = self.clone();
        self.inner.text_view.clipboard().read_text_async(gio::Cancellable::NONE, move |res| {
            if let Ok(Some(text)) = res {
                this.dispatch(Command::Paste(text.to_string()));
            }
        });
    }

    /// Connect the engine and all GTK controllers.
    pub(super) fn wire(&self) {
        let inner = &self.inner;
        let tv = &inner.text_view;

        // Engine -> widgets
        let weak = Rc::downgrade(inner);
        inner.editor.borrow_mut().register_update_listener(move |ev| {
            if let Some(inner) = weak.upgrade() {
                inner.on_update(ev);
            }
        });

        // Keyboard + input method
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.set_im_context(Some(&inner.im));
        let this = self.clone();
        keys.connect_key_pressed(move |_, key, _code, state| {
            if this.handle_key(key, state) {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        tv.add_controller(keys);
        inner.im.set_client_widget(Some(tv));
        let this = self.clone();
        inner.im.connect_commit(move |_, text| {
            this.dispatch(Command::InsertText(text.to_string()));
        });
        let focus = gtk::EventControllerFocus::new();
        let im = inner.im.clone();
        focus.connect_enter(move |_| im.focus_in());
        let im = inner.im.clone();
        focus.connect_leave(move |_| im.focus_out());
        tv.add_controller(focus);

        // Widget -> engine: native selection changes
        let weak = Rc::downgrade(inner);
        inner.buffer.connect_mark_set(move |_, _, mark| {
            if !matches!(mark.name().as_deref(), Some("insert" | "selection_bound")) {
                return;
            }
            if let Some(inner) = weak.upgrade() {
                inner.selection_from_buffer();
            }
        });

        // Clicking a checklist marker toggles the item.
        let click = gtk::GestureClick::new();
        let weak = Rc::downgrade(inner);
        click.connect_pressed(move |_, n_press, x, y| {
            let Some(inner) = weak.upgrade() else { return };
            if n_press == 1 {
                inner.click_marker(x, y);
            }
        });
        tv.add_controller(click);
    }
}

trait WordDelete {
    /// Delete by word instead of by character when `word` is set (Ctrl held).
    fn or_word(self, word: bool) -> Command;
}

impl WordDelete for Command {
    fn or_word(self, word: bool) -> Command {
        match (self, word) {
            (Command::DeleteCharacter { backward }, true) => Command::DeleteWord { backward },
            (c, _) => c,
        }
    }
}
