//! Collaboration demo: two editors, Alice and Bob, share one document through
//! `lexical-sync` (a Loro CRDT). Type in either pane and the other follows. Switch a peer
//! offline, edit both sides, then bring it back online: the edits merge without conflicts.
//! `cargo run -p lexical-adw --example collab`

use adw::gtk::{self, prelude::*};
use lexical_adw::LexicalView;
use lexical_core::{Command, Editor};
use lexical_sync::Collab;
use std::cell::Cell;
use std::rc::Rc;

const APP_ID: &str = "io.github.sevki.LexicalDemo.Collab";

struct Peer {
    name: &'static str,
    view: LexicalView,
    collab: Collab,
    online: Cell<bool>,
}

/// Deliver everything `from` has that `to` lacks.
fn catch_up(from: &Peer, to: &Peer) {
    let version = to.collab.version();
    match from.collab.updates_since(&version) {
        Ok(update) => {
            if let Err(e) = to.collab.receive(&mut to.view.editor().borrow_mut(), &update) {
                eprintln!("{} -> {}: {e}", from.name, to.name);
            }
        }
        Err(e) => eprintln!("{} -> {}: {e}", from.name, to.name),
    }
}

/// Forward `from`'s fresh local edits to `to` while both are online. Offline edits stay in
/// the CRDT and go out on reconnect.
fn push(from: &Peer, to: &Peer) {
    let updates = from.collab.drain_updates();
    if from.online.get() && to.online.get() {
        for update in updates {
            if let Err(e) = to.collab.receive(&mut to.view.editor().borrow_mut(), &update) {
                eprintln!("{} -> {}: {e}", from.name, to.name);
            }
        }
    }
}

fn pane(peer: &Rc<Peer>, other: &Rc<Peer>) -> gtk::Box {
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header.set_margin_start(12);
    header.set_margin_end(12);
    header.set_margin_top(8);
    header.set_margin_bottom(8);
    let title = gtk::Label::builder().label(peer.name).hexpand(true).xalign(0.0).build();
    title.add_css_class("heading");
    let status = gtk::Label::new(Some("online"));
    status.add_css_class("dim-label");
    let switch = gtk::Switch::builder().active(true).valign(gtk::Align::Center).build();
    header.append(&title);
    header.append(&status);
    header.append(&switch);

    {
        let (peer, other, status) = (peer.clone(), other.clone(), status.clone());
        switch.connect_active_notify(move |s| {
            peer.online.set(s.is_active());
            status.set_text(if s.is_active() { "online" } else { "offline" });
            if s.is_active() && other.online.get() {
                catch_up(&peer, &other);
                catch_up(&other, &peer);
            }
        });
    }
    {
        let (peer, other) = (peer.clone(), other.clone());
        peer.clone().view.connect_changed(move |_| push(&peer, &other));
    }

    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.set_hexpand(true);
    column.append(&header);
    column.append(peer.view.toolbar());
    column.append(peer.view.widget());
    column
}

fn build_ui(app: &adw::Application) {
    let mut alice_editor = Editor::new();
    alice_editor.dispatch(Command::Paste(
        "Shared notes\nEdit here, or in the other pane. Turn a peer offline, edit both, then bring it back.".into(),
    ));
    let alice_collab = Collab::attach(&mut alice_editor, 1).expect("attach");
    let snapshot = alice_collab.snapshot().expect("snapshot");
    let mut bob_editor = Editor::new();
    let bob_collab = Collab::join(&mut bob_editor, 2, &snapshot).expect("join");

    let make = |name, editor, collab| {
        Rc::new(Peer { name, view: LexicalView::with_editor(editor), collab, online: Cell::new(true) })
    };
    let alice = make("Alice", alice_editor, alice_collab);
    let bob = make("Bob", bob_editor, bob_collab);

    let panes = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    panes.append(&pane(&alice, &bob));
    panes.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    panes.append(&pane(&bob, &alice));

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&adw::HeaderBar::new());
    toolbar_view.set_content(Some(&panes));
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Lexical collaboration")
        .default_width(1100)
        .default_height(640)
        .content(&toolbar_view)
        .build();
    window.present();
    alice.view.text_view().grab_focus();
}

fn main() -> adw::glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}
