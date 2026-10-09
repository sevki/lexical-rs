//! The jetstream service every peer exposes.

use jetstream::prelude::*;

/// Answer to `hello`: the callee's version and what the caller is missing.
#[derive(Debug, Clone, Default, JetStreamWireFormat)]
pub struct Handshake {
    /// The sender's version, opaque to this crate (`Collab::version`).
    pub version: Vec<u8>,
    /// Updates the receiver lacks (`Collab::updates_since`), possibly empty.
    pub updates: Vec<u8>,
}

#[service]
pub trait Replicate {
    /// Introduce `ticket` (the caller) at `version`; answered with the callee's side.
    async fn hello(&self, ctx: Context, ticket: String, version: Vec<u8>) -> Result<crate::service::Handshake>;
    /// Deliver one live update.
    async fn push(&self, ctx: Context, update: Vec<u8>) -> Result<String>;
}

pub use replicate_protocol::{ReplicateChannel, ReplicateService};
