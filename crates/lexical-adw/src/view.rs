//! [`LexicalView`]: a `GtkTextView`-backed rich text editor driven by `lexical-core`.
//!
//! The text view is read-only from GTK's point of view. All edits arrive as
//! [`Command`]s (from the input method, key bindings, the toolbar or the clipboard),
//! mutate the [`Editor`], and the resulting state is reconciled back into the
//! `GtkTextBuffer`. Native selection changes flow the other way.

use crate::reconciler::{self, Tags};
use crate::toolbar::Toolbar;
use adw::gtk::{self, gdk, gio, glib, prelude::*};
use lexical_core::{
    BlockStyle, Command, Editor, EditorState, Layout, ListType, Result, TextFormat, UpdateEvent,
};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

type ChangeListener = Rc<dyn Fn(&EditorState)>;

struct Inner {
    editor: RefCell<Editor>,
    scroll: gtk::ScrolledWindow,
    text_view: gtk::TextView,
    buffer: gtk::TextBuffer,
    tags: Tags,
    im: gtk::IMMulticontext,
    toolbar: Toolbar,
    layout: RefCell<Layout>,
    /// True while we write to the buffer, so its signals are not fed back.
    syncing: Cell<bool>,
    listeners: RefCell<Vec<ChangeListener>>,
}

#[derive(Clone)]
pub struct LexicalView {
    inner: Rc<Inner>,
}

impl Default for LexicalView {
    fn default() -> Self {
        Self::new()
    }
}

impl LexicalView {
    pub fn new() -> LexicalView {
        Self::with_editor(Editor::new())
    }

    pub fn with_editor(editor: Editor) -> LexicalView {
        let buffer = gtk::TextBuffer::new(None);
        buffer.set_enable_undo(false);
        let text_view = gtk::TextView::with_buffer(&buffer);
        text_view.set_editable(false);
        text_view.set_cursor_visible(true);
        text_view.set_accepts_tab(false);
        text_view.set_wrap_mode(gtk::WrapMode::WordChar);
        text_view.set_left_margin(16);
        text_view.set_right_margin(16);
        text_view.set_top_margin(12);
        text_view.set_bottom_margin(12);
        text_view.set_pixels_below_lines(4);
        text_view.add_css_class("lexical-editor");
        let scroll = gtk::ScrolledWindow::builder()
            .child(&text_view)
            .hexpand(true)
            .vexpand(true)
            .build();
        let tags = Tags::install(&buffer);
        let im = gtk::IMMulticontext::new();

        let inner = Rc::new_cyclic(|weak: &Weak<Inner>| {
            let w = weak.clone();
            let toolbar = Toolbar::new(move |cmd| {
                if let Some(inner) = w.upgrade() {
                    LexicalView { inner }.dispatch(cmd);
                }
            });
            Inner {
                editor: RefCell::new(editor),
                scroll,
                text_view,
                buffer,
                tags,
                im,
                toolbar,
                layout: RefCell::new(Layout::default()),
                syncing: Cell::new(false),
                listeners: RefCell::new(vec![]),
            }
        });
        let view = LexicalView { inner };
        view.wire();
        view.refresh_all();
        view
    }

    // ------------------------------------------------------------------ public

    /// The scrolled editing surface.
    pub fn widget(&self) -> &gtk::Widget {
        self.inner.scroll.upcast_ref()
    }

    /// Formatting toolbar; place it in an `adw::ToolbarView` top bar.
    pub fn toolbar(&self) -> &gtk::Widget {
        self.inner.toolbar.widget()
    }

    pub fn text_view(&self) -> &gtk::TextView {
        &self.inner.text_view
    }

    pub fn buffer(&self) -> &gtk::TextBuffer {
        &self.inner.buffer
    }

    /// Direct access to the engine. Do not hold the borrow across GTK calls that
    /// could re-enter the view.
    pub fn editor(&self) -> &RefCell<Editor> {
        &self.inner.editor
    }

    /// Run a command; returns whether it was handled.
    pub fn dispatch(&self, cmd: Command) -> bool {
        match self.inner.editor.try_borrow_mut() {
            Ok(mut e) => e.dispatch(cmd),
            Err(_) => false,
        }
    }

    /// Called after every committed update with the new state.
    pub fn connect_changed(&self, f: impl Fn(&EditorState) + 'static) {
        self.inner.listeners.borrow_mut().push(Rc::new(f));
    }

    pub fn set_editable(&self, editable: bool) {
        self.inner.editor.borrow_mut().set_editable(editable);
    }

    pub fn load_json(&self, json: &str) -> Result<()> {
        let state = EditorState::from_json_str(json)?;
        self.inner.editor.borrow_mut().set_state(state);
        Ok(())
    }

    pub fn to_json(&self) -> String {
        self.inner.editor.borrow().state().to_json_string()
    }

    /// The input-method context used for typing; exposed for tests.
    #[doc(hidden)]
    pub fn input_method(&self) -> &gtk::IMContext {
        self.inner.im.upcast_ref()
    }

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
            if ctrl { Command::DeleteWord { backward: true } } else { Command::DeleteCharacter { backward: true } }
        } else if is(gdk::Key::Delete) || is(gdk::Key::KP_Delete) {
            if ctrl { Command::DeleteWord { backward: false } } else { Command::DeleteCharacter { backward: false } }
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

    // ---------------------------------------------------------------- internals

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

    fn wire(&self) {
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

    fn refresh_all(&self) {
        let inner = &self.inner;
        let ed = inner.editor.borrow();
        let state = ed.state();
        let layout = Layout::build(state);
        inner.syncing.set(true);
        reconciler::apply(&inner.buffer, &inner.tags, &layout);
        inner.apply_selection(state, &layout);
        *inner.layout.borrow_mut() = layout;
        inner.syncing.set(false);
        inner.toolbar.sync(state, ed.can_undo(), ed.can_redo());
    }
}

impl Inner {
    fn on_update(&self, ev: &UpdateEvent) {
        let layout = if ev.content_changed() || ev.has_tag(lexical_core::Tag::Historic) {
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

    fn apply_selection(&self, state: &EditorState, layout: &Layout) {
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

    fn selection_from_buffer(&self) {
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

    fn click_marker(&self, x: f64, y: f64) {
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
