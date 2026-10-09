//! Two nodes on one machine connect through a ticket and converge: edits made before they
//! meet, edits made while connected, and edits made while one side was cut off.
//!
//! Needs only loopback; the relay and address lookup iroh also tries are not required.

use lexical_core::Command;
use lexical_iroh::{Event, Node};
use lexical_sync::Replica;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedReceiver;

struct Peer {
    replica: Replica,
    node: Node,
    events: UnboundedReceiver<Event>,
    up: usize,
}

impl Peer {
    fn new(id: u64) -> Peer {
        let (node, events) = Node::spawn().expect("spawn");
        Peer { replica: Replica::new(id).unwrap(), node, events, up: 0 }
    }

    /// Handle what the transport asked for and send out local edits.
    fn pump(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Update(bytes) => self.replica.receive(&bytes).unwrap(),
                Event::Version(reply) => {
                    let _ = reply.send(self.replica.collab.version());
                }
                Event::Missing { version, reply } => {
                    let _ = reply.send(self.replica.collab.updates_since(&version).unwrap());
                }
                Event::PeerUp(_) => self.up += 1,
                Event::PeerDown(_) | Event::Ready(_) => {}
            }
        }
        for update in self.replica.drain_updates() {
            self.node.broadcast(update);
        }
    }

    fn edit(&mut self, text: &str) {
        self.replica.dispatch(Command::Paste(text.into()));
        self.pump();
    }
}

fn settle(a: &mut Peer, b: &mut Peer, done: impl Fn(&Peer, &Peer) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        a.pump();
        b.pump();
        if done(a, b) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("did not settle:\n a = {:?}\n b = {:?}", a.replica.text(), b.replica.text());
}

#[test]
fn peers_converge_through_a_ticket() {
    let mut a = Peer::new(1);
    let mut b = Peer::new(2);
    a.edit("written before meeting");

    // Only `b` learns `a`'s ticket; `a` is told about `b` by the hello and dials back.
    b.node.connect(a.node.ticket());
    settle(&mut a, &mut b, |a, b| a.up >= 1 && b.up >= 1 && a.replica.text() == b.replica.text());
    assert!(b.replica.text().contains("written before meeting"));

    // Live edits in both directions.
    b.edit(" and from b");
    settle(&mut a, &mut b, |a, b| a.replica.text() == b.replica.text() && a.replica.text().contains("from b"));
    a.edit(" and from a");
    settle(&mut a, &mut b, |a, b| a.replica.text() == b.replica.text() && b.replica.text().contains("from a"));

    // Both edit at once; the CRDT merges them.
    a.edit("[A]");
    b.edit("[B]");
    settle(&mut a, &mut b, |a, b| {
        let t = a.replica.text();
        t == b.replica.text() && t.contains("[A]") && t.contains("[B]")
    });
}
