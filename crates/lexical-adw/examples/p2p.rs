//! Peer-to-peer collaboration demo: one editor that shares its document with other devices
//! over iroh, carried by jetstream (`lexical-iroh`). No server: press *Share* on one device
//! and paste the ticket into *Join* on another. Either side can go offline and come back; the
//! CRDT merges what was typed meanwhile.
//! `cargo run -p lexical-adw --example p2p`

use adw::gtk::{self, glib, prelude::*};
use lexical_adw::LexicalView;
use lexical_core::Editor;
use lexical_iroh::{Event, Node, Ticket};
use lexical_sync::Collab;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const APP_ID: &str = "io.github.sevki.LexicalDemo.P2p";

struct Session {
    view: LexicalView,
    collab: Collab,
    node: Node,
    ticket: RefCell<Ticket>,
    peers: Cell<usize>,
}

fn status_text(peers: usize) -> String {
    match peers {
        0 => "alone".into(),
        1 => "1 peer".into(),
        n => format!("{n} peers"),
    }
}

/// The formatting toolbar is wider than a phone screen, so let it scroll sideways.
fn scrolled_toolbar(view: &LexicalView) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .child(view.toolbar())
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .build()
}

fn share_popover(session: &Rc<Session>) -> gtk::Popover {
    let label = gtk::Label::builder()
        .label(session.ticket.borrow().to_string())
        .selectable(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::Char)
        .max_width_chars(36)
        .build();
    label.add_css_class("monospace");
    let copy = gtk::Button::with_label("Copy ticket");
    {
        let (session, label) = (session.clone(), label.clone());
        copy.connect_clicked(move |b| {
            let text = session.ticket.borrow().to_string();
            label.set_text(&text);
            b.clipboard().set_text(&text);
        });
    }
    let hint = gtk::Label::builder()
        .label("Paste this ticket into Join on the other device.")
        .xalign(0.0)
        .wrap(true)
        .max_width_chars(36)
        .build();
    hint.add_css_class("dim-label");
    let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
    column.set_margin_start(12);
    column.set_margin_end(12);
    column.set_margin_top(12);
    column.set_margin_bottom(12);
    column.append(&hint);
    column.append(&label);
    column.append(&copy);
    let popover = gtk::Popover::new();
    popover.set_child(Some(&column));
    {
        let (session, label) = (session.clone(), label.clone());
        popover.connect_show(move |_| label.set_text(&session.ticket.borrow().to_string()));
    }
    popover
}

fn join_popover(session: &Rc<Session>, toast: &adw::ToastOverlay) -> gtk::Popover {
    let entry = gtk::Entry::builder().placeholder_text("Paste a ticket").width_chars(32).build();
    let go = gtk::Button::with_label("Join");
    go.add_css_class("suggested-action");
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.set_margin_start(12);
    row.set_margin_end(12);
    row.set_margin_top(12);
    row.set_margin_bottom(12);
    row.append(&entry);
    row.append(&go);
    let popover = gtk::Popover::new();
    popover.set_child(Some(&row));

    let connect = {
        let (session, entry, popover, toast) = (session.clone(), entry.clone(), popover.clone(), toast.clone());
        move || {
            let text = entry.text();
            match text.trim().parse::<Ticket>() {
                Ok(ticket) => {
                    session.node.connect(ticket);
                    entry.set_text("");
                    popover.popdown();
                    toast.add_toast(adw::Toast::new("Connecting…"));
                }
                Err(e) => toast.add_toast(adw::Toast::new(&format!("Not a valid ticket: {e}"))),
            }
        }
    };
    let c = connect.clone();
    go.connect_clicked(move |_| c());
    entry.connect_activate(move |_| connect());
    popover
}

fn build_ui(app: &adw::Application) {
    let (node, mut events) = match Node::spawn() {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("cannot start the network node: {e}");
            return;
        }
    };
    let mut editor = Editor::new();
    // Peer ids only need to be distinct; the node's public key already is.
    let peer = u64::from_le_bytes(node.id().as_bytes()[..8].try_into().unwrap()) | 1;
    let collab = Collab::attach(&mut editor, peer).expect("attach");
    let session = Rc::new(Session {
        view: LexicalView::with_editor(editor),
        collab,
        ticket: RefCell::new(node.ticket()),
        node,
        peers: Cell::new(0),
    });

    let status = gtk::Label::new(Some(&status_text(0)));
    status.add_css_class("dim-label");

    // Local edits go out to every peer.
    {
        let session = session.clone();
        session.clone().view.connect_changed(move |_| {
            for update in session.collab.drain_updates() {
                session.node.broadcast(update);
            }
        });
    }

    let toast = adw::ToastOverlay::new();
    let share = gtk::MenuButton::builder().label("Share").popover(&share_popover(&session)).build();
    let join = gtk::MenuButton::builder().label("Join").popover(&join_popover(&session, &toast)).build();
    let header = adw::HeaderBar::new();
    header.pack_start(&share);
    header.pack_start(&join);
    header.pack_end(&status);

    // What the network asks of the document runs here, on the GTK thread.
    {
        let (session, status) = (session.clone(), status.clone());
        glib::spawn_future_local(async move {
            while let Some(event) = events.recv().await {
                match event {
                    Event::Ready(ticket) => *session.ticket.borrow_mut() = ticket,
                    Event::Update(bytes) => {
                        let result = session.collab.receive(&mut session.view.editor().borrow_mut(), &bytes);
                        if let Err(e) = result {
                            eprintln!("bad update: {e}");
                        }
                    }
                    Event::Version(reply) => {
                        let _ = reply.send(session.collab.version());
                    }
                    Event::Missing { version, reply } => match session.collab.updates_since(&version) {
                        Ok(update) => {
                            let _ = reply.send(update);
                        }
                        Err(e) => eprintln!("cannot encode updates: {e}"),
                    },
                    Event::PeerUp(_) => {
                        session.peers.set(session.peers.get() + 1);
                        status.set_text(&status_text(session.peers.get()));
                    }
                    Event::PeerDown(_) => {
                        session.peers.set(session.peers.get().saturating_sub(1));
                        status.set_text(&status_text(session.peers.get()));
                    }
                }
            }
        });
    }

    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.append(&scrolled_toolbar(&session.view));
    column.append(session.view.widget());
    toast.set_child(Some(&column));

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&header);
    toolbar_view.set_content(Some(&toast));
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Lexical peer-to-peer")
        .default_width(720)
        .default_height(640)
        .content(&toolbar_view)
        .build();
    window.set_width_request(320);
    window.present();
    session.view.text_view().grab_focus();
    // Keep the node (and its runtime) alive as long as the window.
    window.connect_destroy(move |_| drop(session.clone()));
}

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}
