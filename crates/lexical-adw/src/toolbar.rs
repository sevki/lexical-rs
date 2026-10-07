//! Formatting toolbar. Pure widgets: it emits [`Command`]s through a callback and
//! is refreshed from editor state via [`Toolbar::sync`].

use adw::gtk;
use adw::gtk::prelude::*;
use lexical_core::{
    Align, BlockType, Command, EditorState, HeadingTag, ListType, NodeData, TextFormat,
};
use std::cell::Cell;
use std::rc::Rc;

const BLOCKS: [(&str, BlockType); 8] = [
    ("Paragraph", BlockType::Paragraph),
    ("Heading 1", BlockType::Heading(HeadingTag::H1)),
    ("Heading 2", BlockType::Heading(HeadingTag::H2)),
    ("Heading 3", BlockType::Heading(HeadingTag::H3)),
    ("Heading 4", BlockType::Heading(HeadingTag::H4)),
    ("Heading 5", BlockType::Heading(HeadingTag::H5)),
    ("Quote", BlockType::Quote),
    ("Code Block", BlockType::Code),
];

pub struct Toolbar {
    root: gtk::Box,
    undo: gtk::Button,
    redo: gtk::Button,
    blocks: gtk::DropDown,
    formats: Vec<(TextFormat, gtk::ToggleButton)>,
    lists: Vec<(ListType, gtk::ToggleButton)>,
    aligns: Vec<(Align, gtk::ToggleButton)>,
    link_button: gtk::MenuButton,
    link_entry: gtk::Entry,
    /// Suppresses command emission while the toolbar mirrors editor state.
    syncing: Rc<Cell<bool>>,
}

fn group() -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.add_css_class("linked");
    b
}

impl Toolbar {
    pub fn new(emit: impl Fn(Command) + 'static) -> Toolbar {
        let emit: Rc<dyn Fn(Command)> = Rc::new(emit);
        let syncing = Rc::new(Cell::new(false));
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        root.set_margin_start(6);
        root.set_margin_end(6);
        root.set_margin_top(3);
        root.set_margin_bottom(3);
        root.set_halign(gtk::Align::Start);

        let button = |icon: &str, tip: &str, cmd: Command| {
            let b = gtk::Button::from_icon_name(icon);
            b.set_tooltip_text(Some(tip));
            let emit = emit.clone();
            b.connect_clicked(move |_| emit(cmd.clone()));
            b
        };
        let toggle = |icon: Option<&str>, label: Option<&str>, tip: &str, cmd: Command| {
            let b = gtk::ToggleButton::new();
            if let Some(i) = icon {
                b.set_icon_name(i);
            }
            if let Some(l) = label {
                b.set_label(l);
            }
            b.set_tooltip_text(Some(tip));
            let (emit, syncing) = (emit.clone(), syncing.clone());
            b.connect_toggled(move |_| {
                if !syncing.get() {
                    emit(cmd.clone());
                }
            });
            b
        };

        // History
        let hist = group();
        let undo = button("edit-undo-symbolic", "Undo (Ctrl+Z)", Command::Undo);
        let redo = button("edit-redo-symbolic", "Redo (Ctrl+Shift+Z)", Command::Redo);
        hist.append(&undo);
        hist.append(&redo);
        root.append(&hist);

        // Block type
        let names: Vec<&str> = BLOCKS.iter().map(|b| b.0).collect();
        let blocks = gtk::DropDown::from_strings(&names);
        blocks.set_tooltip_text(Some("Block type"));
        {
            let (emit, syncing) = (emit.clone(), syncing.clone());
            blocks.connect_selected_notify(move |d| {
                if !syncing.get()
                    && let Some((_, ty)) = BLOCKS.get(d.selected() as usize) {
                        emit(Command::SetBlockType(*ty));
                    }
            });
        }
        root.append(&blocks);

        // Inline formats
        let fmt_group = group();
        let mut formats = vec![];
        for (flag, icon, label, tip) in [
            (TextFormat::BOLD, Some("format-text-bold-symbolic"), None, "Bold (Ctrl+B)"),
            (TextFormat::ITALIC, Some("format-text-italic-symbolic"), None, "Italic (Ctrl+I)"),
            (TextFormat::UNDERLINE, Some("format-text-underline-symbolic"), None, "Underline (Ctrl+U)"),
            (TextFormat::STRIKETHROUGH, Some("format-text-strikethrough-symbolic"), None, "Strikethrough"),
            (TextFormat::CODE, None, Some("</>"), "Inline code"),
        ] {
            let b = toggle(icon, label, tip, Command::FormatText(flag));
            fmt_group.append(&b);
            formats.push((flag, b));
        }
        root.append(&fmt_group);

        // Link
        let link_button = gtk::MenuButton::new();
        link_button.set_icon_name("insert-link-symbolic");
        link_button.set_tooltip_text(Some("Link"));
        let link_entry = gtk::Entry::builder().placeholder_text("https://…").width_chars(28).build();
        let apply = gtk::Button::with_label("Apply");
        apply.add_css_class("suggested-action");
        let remove = gtk::Button::with_label("Remove");
        let pop_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        for w in [link_entry.upcast_ref::<gtk::Widget>(), apply.upcast_ref(), remove.upcast_ref()] {
            pop_box.append(w);
        }
        pop_box.set_margin_top(6);
        pop_box.set_margin_bottom(6);
        pop_box.set_margin_start(6);
        pop_box.set_margin_end(6);
        let popover = gtk::Popover::new();
        popover.set_child(Some(&pop_box));
        link_button.set_popover(Some(&popover));
        {
            let (emit, entry, pop) = (emit.clone(), link_entry.clone(), popover.clone());
            let go = move || {
                let url = entry.text().trim().to_string();
                emit(Command::ToggleLink((!url.is_empty()).then_some(url)));
                pop.popdown();
            };
            let go2 = go.clone();
            apply.connect_clicked(move |_| go());
            link_entry.connect_activate(move |_| go2());
        }
        {
            let (emit, pop) = (emit.clone(), popover.clone());
            remove.connect_clicked(move |_| {
                emit(Command::ToggleLink(None));
                pop.popdown();
            });
        }
        root.append(&link_button);

        // Lists
        let list_group = group();
        let mut lists = vec![];
        for (ty, icon, tip) in [
            (ListType::Bullet, "view-list-bullet-symbolic", "Bulleted list"),
            (ListType::Number, "view-list-ordered-symbolic", "Numbered list"),
            (ListType::Check, "checkbox-checked-symbolic", "Checklist"),
        ] {
            let b = toggle(Some(icon), None, tip, Command::ToggleList(ty));
            list_group.append(&b);
            lists.push((ty, b));
        }
        root.append(&list_group);

        // Alignment
        let align_group = group();
        let mut aligns = vec![];
        for (a, icon, tip) in [
            (Align::Left, "format-justify-left-symbolic", "Align left"),
            (Align::Center, "format-justify-center-symbolic", "Align center"),
            (Align::Right, "format-justify-right-symbolic", "Align right"),
        ] {
            let b = toggle(Some(icon), None, tip, Command::FormatAlign(a));
            align_group.append(&b);
            aligns.push((a, b));
        }
        root.append(&align_group);

        // Indent
        let indent_group = group();
        indent_group.append(&button("format-indent-less-symbolic", "Outdent (Shift+Tab)", Command::Outdent));
        indent_group.append(&button("format-indent-more-symbolic", "Indent (Tab)", Command::Indent));
        root.append(&indent_group);

        Toolbar { root, undo, redo, blocks, formats, lists, aligns, link_button, link_entry, syncing }
    }

    pub fn widget(&self) -> &gtk::Widget {
        self.root.upcast_ref()
    }

    /// Mirror editor state into the controls without emitting commands.
    pub fn sync(&self, state: &EditorState, can_undo: bool, can_redo: bool) {
        self.syncing.set(true);
        self.undo.set_sensitive(can_undo);
        self.redo.set_sensitive(can_redo);
        let fmt = state.selection_format();
        for (flag, b) in &self.formats {
            b.set_active(fmt.contains(*flag));
        }
        let block = state.current_block();
        let (mut list_ty, mut idx, mut align) = (None, 0u32, Align::Start);
        if let Some(b) = block {
            let n = state.node(b);
            align = n.align;
            match &n.data {
                // The dropdown offers H1-H5; deeper headings show as H5.
                NodeData::Heading(t) => idx = (t.level() as u32).min(5),
                NodeData::Quote => idx = 6,
                NodeData::Code { .. } => idx = 7,
                NodeData::ListItem { .. } => list_ty = state.list_type_of_item(b),
                _ => {}
            }
        }
        self.blocks.set_selected(idx.min(7));
        self.blocks.set_sensitive(list_ty.is_none());
        for (ty, b) in &self.lists {
            b.set_active(list_ty == Some(*ty));
        }
        for (a, b) in &self.aligns {
            b.set_active(align == *a || (*a == Align::Left && align == Align::Start));
        }
        let url = state.link_at_selection();
        self.link_button.set_icon_name(if url.is_some() { "emblem-ok-symbolic" } else { "insert-link-symbolic" });
        self.link_entry.set_text(url.as_deref().unwrap_or(""));
        self.syncing.set(false);
    }
}
