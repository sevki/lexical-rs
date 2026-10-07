//! Playground: a libadwaita window with the editor, formatting toolbar and a live
//! view of the serialized Lexical JSON. `cargo run -p lexical-adw --example demo`

use adw::prelude::*;
use adw::gtk::{self, gio};
use lexical_adw::LexicalView;
use lexical_core::{Command, Editor, MarkdownShortcutsPlugin};

const APP_ID: &str = "io.github.sevki.LexicalDemo";

fn build_ui(app: &adw::Application) {
    let mut editor = Editor::new();
    editor.add_plugin(Box::new(MarkdownShortcutsPlugin::default()));
    let view = LexicalView::with_editor(editor);
    view.dispatch(Command::Paste(
        "Welcome to Lexical on libadwaita\nTry the toolbar, or type “# ”, “- ” or “1. ” at the start of a line.".into(),
    ));

    // JSON inspector sidebar
    let json_view = gtk::TextView::builder()
        .editable(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .left_margin(8)
        .right_margin(8)
        .build();
    let json_scroll = gtk::ScrolledWindow::builder().child(&json_view).min_content_width(320).build();
    {
        let jv = json_view.clone();
        view.connect_changed(move |s| jv.buffer().set_text(&s.to_json_string()));
        json_view.buffer().set_text(&view.to_json());
    }

    let split = adw::OverlaySplitView::new();
    split.set_sidebar_position(gtk::PackType::End);
    split.set_sidebar(Some(&json_scroll));
    split.set_content(Some(view.widget()));
    split.set_show_sidebar(false);

    let toolbar_view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    let json_toggle = gtk::ToggleButton::builder()
        .icon_name("sidebar-show-right-symbolic")
        .tooltip_text("Show editor state (JSON)")
        .build();
    json_toggle.bind_property("active", &split, "show-sidebar").bidirectional().build();
    header.pack_end(&json_toggle);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Lexical")
        .default_width(900)
        .default_height(640)
        .build();

    let open = gtk::Button::from_icon_name("document-open-symbolic");
    open.set_tooltip_text(Some("Open Lexical JSON"));
    let save = gtk::Button::from_icon_name("document-save-symbolic");
    save.set_tooltip_text(Some("Save as Lexical JSON"));
    header.pack_start(&open);
    header.pack_start(&save);
    {
        let (view, window) = (view.clone(), window.clone());
        open.connect_clicked(move |_| {
            let (view, win) = (view.clone(), window.clone());
            gtk::FileDialog::new().open(Some(&window), gio::Cancellable::NONE, move |res| {
                let Ok(file) = res else { return };
                match file.load_contents(gio::Cancellable::NONE) {
                    Ok((bytes, _)) => {
                        if let Err(e) = view.load_json(&String::from_utf8_lossy(&bytes)) {
                            toast(&win, &format!("Could not load: {e}"));
                        }
                    }
                    Err(e) => toast(&win, &format!("Could not read: {e}")),
                }
            });
        });
    }
    {
        let (view, window) = (view.clone(), window.clone());
        save.connect_clicked(move |_| {
            let (view, win) = (view.clone(), window.clone());
            gtk::FileDialog::new().save(Some(&window), gio::Cancellable::NONE, move |res| {
                let Ok(file) = res else { return };
                if let Err(e) = file.replace_contents(
                    view.to_json().as_bytes(),
                    None,
                    false,
                    gio::FileCreateFlags::NONE,
                    gio::Cancellable::NONE,
                ) {
                    toast(&win, &format!("Could not save: {e}"));
                }
            });
        });
    }

    toolbar_view.add_top_bar(&header);
    toolbar_view.add_top_bar(view.toolbar());
    toolbar_view.set_content(Some(&split));
    window.set_content(Some(&toolbar_view));
    window.present();
    view.text_view().grab_focus();
}

fn toast(window: &adw::ApplicationWindow, msg: &str) {
    let dialog = adw::AlertDialog::new(Some("Lexical"), Some(msg));
    dialog.add_response("ok", "OK");
    dialog.present(Some(window));
}

fn main() -> adw::glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}
