//! Peer-to-peer document sync over [iroh](https://iroh.computer), carried by
//! [jetstream](https://jetstream.rs) RPC instead of a websocket server.
//!
//! Every peer is both a server and a client of the same two-call service ([`Replicate`]):
//!
//! * `hello(ticket, version)` introduces a peer. The callee answers with its own version
//!   and whatever the caller is missing; the caller then sends back whatever the callee is
//!   missing. That is the anti-entropy step, and it also repairs a gap after the network
//!   dropped.
//! * `push(update)` delivers one live update.
//!
//! The payload is opaque bytes, so it carries `lexical-sync` (Loro) updates as well as
//! y-protocols messages from `lexical-yjs`. A peer that receives a `hello` from someone it
//! does not know dials them back, so a single [`Ticket`] pasted on one side connects both
//! directions. Peers find each other by node id through iroh's relay and discovery, so it
//! works across NATs, on a phone included.
//!
//! The crate is GUI-free and does not need the caller to run tokio: [`Node::spawn`] runs
//! its own runtime on a thread, and the caller sees plain channels. The document types
//! (`Collab` is `!Send`) stay on the caller's thread:
//!
//! ```ignore
//! let (node, mut events) = Node::spawn()?;
//! println!("{}", node.ticket());               // share this
//! node.connect(other_ticket);                  // or join someone else's
//! node.broadcast(update);                      // each local update from drain_updates()
//! while let Some(event) = events.recv().await { // on the thread that owns the editor
//!     match event {
//!         Event::Update(bytes) => collab.receive(&mut editor, &bytes)?,
//!         Event::Catchup { version, reply } => { let _ = reply.send(collab.updates_since(&version)?); }
//!         Event::Version { reply } => { let _ = reply.send(collab.version()); }
//!         Event::PeerUp(id) | Event::PeerDown(id) => ..,
//!     }
//! }
//! ```

mod node;
mod service;
mod transport;

pub use node::{EndpointId, Event, Node, Ticket};
pub use service::Replicate;

/// Errors starting or talking to a [`Node`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not start the iroh endpoint: {0}")]
    Endpoint(String),
    #[error("invalid ticket: {0}")]
    Ticket(String),
    #[error("the node has shut down")]
    Closed,
}

pub type Result<T> = std::result::Result<T, Error>;
