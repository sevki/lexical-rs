//! [`LexicalView`]: a `GtkTextView`-backed rich text editor driven by `lexical-core`.
//!
//! The text view is read-only from GTK's point of view. All edits arrive as
//! [`Command`]s (from the input method, key bindings, the toolbar or the clipboard),
//! mutate the [`Editor`], and the resulting state is reconciled back into the
//! `GtkTextBuffer`. Native selection changes flow the other way.
//!
//! * `input` – key bindings, input method and clipboard
//! * `sync` – keeping the widget and the engine selection/content in step

mod input;
mod sync;

use crate::reconciler::{self, Tags};
use crate::toolbar::Toolbar;
use adw::gtk::{self, prelude::*};
use lexical_core::{Command, Editor, EditorState, Layout, Result};
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
