//! Headless GTK/libadwaita tests. `harness = false`: GTK must be initialised and used
//! on the main thread, which the default multi-threaded test harness does not allow.
//! Run under a display server, e.g. `xvfb-run -a cargo test -p lexical-adw`.

use adw::gtk::{self, gdk, prelude::*};
use adw::prelude::*;
use lexical_adw::LexicalView;
use lexical_core::{Command, EditorState, HeadingTag, BlockType};

fn text(v: &LexicalView) -> String {
    let b = v.buffer();
    let (s, e) = b.bounds();
    b.text(&s, &e, false).to_string()
}

fn has_tag_at(v: &LexicalView, name: &str, offset: i32) -> bool {
    let tag = v.buffer().tag_table().lookup(name).unwrap_or_else(|| panic!("no tag {name}"));
    v.buffer().iter_at_offset(offset).has_tag(&tag)
}

fn commit(v: &LexicalView, s: &str) {
    v.input_method().emit_by_name::<()>("commit", &[&s]);
}

fn key(v: &LexicalView, k: gdk::Key, mods: gdk::ModifierType) -> bool {
    v.handle_key(k, mods)
}

fn typing_goes_through_the_engine() {
    let v = LexicalView::new();
    commit(&v, "Hello");
    assert_eq!(text(&v), "Hello");
    assert!(key(&v, gdk::Key::Return, gdk::ModifierType::empty()));
    commit(&v, "World");
    assert_eq!(text(&v), "Hello\nWorld");
    // caret follows the engine
    let ins = v.buffer().iter_at_mark(&v.buffer().get_insert()).offset();
    assert_eq!(ins, 11);
    // backspace
    key(&v, gdk::Key::BackSpace, gdk::ModifierType::empty());
    assert_eq!(text(&v), "Hello\nWorl");
}

fn formatting_renders_tags_and_syncs_toolbar() {
    let v = LexicalView::new();
    commit(&v, "bold text");
    let b = v.buffer();
    b.select_range(&b.iter_at_offset(0), &b.iter_at_offset(4)); // native selection -> engine
    assert!(key(&v, gdk::Key::b, gdk::ModifierType::CONTROL_MASK));
    assert!(has_tag_at(&v, "lexical-bold", 1));
    assert!(!has_tag_at(&v, "lexical-bold", 6));
    // selection survived reconciliation
    let (s, e) = b.selection_bounds().expect("selection kept");
    assert_eq!((s.offset(), e.offset()), (0, 4));
    assert!(v.editor().borrow().state().selection_format().contains(lexical_core::TextFormat::BOLD));
}

fn undo_redo_via_keys() {
    let v = LexicalView::new();
    commit(&v, "a");
    key(&v, gdk::Key::Return, gdk::ModifierType::empty());
    commit(&v, "b");
    assert_eq!(text(&v), "a\nb");
    key(&v, gdk::Key::z, gdk::ModifierType::CONTROL_MASK);
    assert_eq!(text(&v), "a\n");
    key(&v, gdk::Key::z, gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK);
    assert_eq!(text(&v), "a\nb");
}

fn block_types_and_lists_render() {
    let v = LexicalView::new();
    commit(&v, "Title");
    v.dispatch(Command::SetBlockType(BlockType::Heading(HeadingTag::H1)));
    assert!(has_tag_at(&v, "lexical-h1", 2));
    key(&v, gdk::Key::Return, gdk::ModifierType::empty());
    commit(&v, "one");
    v.dispatch(Command::ToggleList(lexical_core::ListType::Bullet));
    assert_eq!(text(&v), "Title\n• one");
    assert!(has_tag_at(&v, "lexical-marker", 6));
    // Tab indents the list item, rendered as a nested (deeper margin) item.
    key(&v, gdk::Key::Tab, gdk::ModifierType::empty());
    assert!(has_tag_at(&v, "lexical-margin-48-18", 8));
}

fn native_selection_is_clamped_out_of_list_markers() {
    let v = LexicalView::new();
    commit(&v, "item");
    v.dispatch(Command::ToggleList(lexical_core::ListType::Bullet));
    let b = v.buffer();
    b.place_cursor(&b.iter_at_offset(0)); // inside "• "
    let ins = b.iter_at_mark(&b.get_insert()).offset();
    assert_eq!(ins, 2, "caret is moved past the marker");
}

fn paste_and_json_round_trip() {
    let v = LexicalView::new();
    v.dispatch(Command::Paste("x\ny\nz".into()));
    assert_eq!(text(&v), "x\ny\nz");
    let json = v.to_json();
    let w = LexicalView::new();
    w.load_json(&json).unwrap();
    assert_eq!(text(&w), "x\ny\nz");
    assert_eq!(EditorState::from_json_str(&json).unwrap().to_json_string(), json);
}

fn changed_callback_and_toolbar_widgets() {
    use std::cell::Cell;
    use std::rc::Rc;
    let v = LexicalView::new();
    let n = Rc::new(Cell::new(0));
    let n2 = n.clone();
    v.connect_changed(move |_| n2.set(n2.get() + 1));
    commit(&v, "hi");
    assert!(n.get() >= 1);

    // The toolbar is a real widget tree usable inside an Adw.ToolbarView.
    let tv = adw::ToolbarView::new();
    tv.add_top_bar(v.toolbar());
    tv.set_content(Some(v.widget()));
    let win = adw::Window::new();
    win.set_content(Some(&tv));
    win.present();
    // Toolbar buttons dispatch real commands.
    fn find_button(w: &gtk::Widget, tip: &str) -> Option<gtk::ToggleButton> {
        if let Some(b) = w.downcast_ref::<gtk::ToggleButton>() {
            if b.tooltip_text().as_deref() == Some(tip) {
                return Some(b.clone());
            }
        }
        let mut c = w.first_child();
        while let Some(child) = c {
            if let Some(b) = find_button(&child, tip) {
                return Some(b);
            }
            c = child.next_sibling();
        }
        None
    }
    let b = v.buffer();
    b.select_range(&b.iter_at_offset(0), &b.iter_at_offset(2));
    let bold = find_button(v.toolbar(), "Bold (Ctrl+B)").expect("bold button");
    bold.set_active(true);
    assert!(has_tag_at(&v, "lexical-bold", 0));
    // …and mirror state back without re-dispatching.
    assert!(bold.is_active());
    win.close();
}

fn main() {
    if gtk::init().is_err() {
        eprintln!("no display available; run under xvfb-run (skipping)");
        std::process::exit(if std::env::var_os("CI").is_some() { 1 } else { 0 });
    }
    adw::init().expect("libadwaita init");
    let tests: &[(&str, fn())] = &[
        ("typing_goes_through_the_engine", typing_goes_through_the_engine),
        ("formatting_renders_tags_and_syncs_toolbar", formatting_renders_tags_and_syncs_toolbar),
        ("undo_redo_via_keys", undo_redo_via_keys),
        ("block_types_and_lists_render", block_types_and_lists_render),
        ("native_selection_is_clamped_out_of_list_markers", native_selection_is_clamped_out_of_list_markers),
        ("paste_and_json_round_trip", paste_and_json_round_trip),
        ("changed_callback_and_toolbar_widgets", changed_callback_and_toolbar_widgets),
    ];
    let mut failed = 0;
    for (name, f) in tests {
        match std::panic::catch_unwind(f) {
            Ok(()) => println!("test {name} ... ok"),
            Err(_) => {
                println!("test {name} ... FAILED");
                failed += 1;
            }
        }
    }
    println!("\ntest result: {}. {} passed; {} failed", if failed == 0 { "ok" } else { "FAILED" }, tests.len() - failed, failed);
    if failed > 0 {
        std::process::exit(1);
    }
}
