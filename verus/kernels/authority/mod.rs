//! Authority kernel: server-authoritative, versioned state with optimistic clients.
//! This is the "sync" kernel (cf. `kernels/Authority.dfy` in dafny-replay): a single
//! authority applies actions only through verified transitions, clients may be stale
//! or send garbage, and the server state still always satisfies the domain invariant.
//!
//! Domain obligations:
//!   (A1) `inv(init())`                        -- same as R1
//!   (A2) `inv(m) ==> inv(step(m, a))`         -- same as R2 (`accepts` only filters)
//!
//! Kernel theorems (proved once):
//!   (AK1) for any trace of requests (any client, any versions, any actions)
//!         `inv(server.present)` holds -- `server`,
//!   (AK2) a rejected request (stale or domain-invalid) leaves the server unchanged,
//!   (AK3) the version is exactly the length of the applied log and grows by one
//!         per accepted request, never otherwise,
//!   (AK4) **replay**: the server state is always the fold of the applied log from
//!         `init()` (state = replay of history; the log is a complete record),
//!   (AK5) completeness: a fresh (non-stale) request the domain accepts is applied,
//!   (AK6) clients: optimistic local actions keep the client model valid, and
//!         re-basing pending actions onto a fresh server state keeps it valid -- `client`.
//!
//! This kernel is server-authoritative. Peer-to-peer collaboration (CRDT merge) is a
//! different protocol and is covered by the `lexical-sync` crate's tests rather than here.

mod client;
mod server;

pub use client::*;
pub use server::*;

use crate::kernels::replay::{step, Domain};
use vstd::prelude::*;

verus! {

pub trait AuthDomain: Domain {
    /// Domain-level validity gate (e.g. ranges in bounds). Orthogonal to the invariant.
    spec fn accepts(m: Self::Model, a: Self::Action) -> bool;
}

pub struct Server<D: Domain> {
    pub version: nat,
    pub present: D::Model,
    pub log: Seq<D::Action>,
}

pub enum Reply<M> {
    Accepted { version: nat, model: M },
    /// Client based its request on an old version: it must resync.
    Stale { version: nat, model: M },
    /// Version matched but the domain refused the action.
    Rejected { version: nat, model: M },
}

pub open spec fn init_server<D: Domain>() -> Server<D> {
    Server { version: 0, present: D::init(), log: Seq::empty() }
}

/// Fold of `step` over a log, starting from the initial model.
pub open spec fn replay_log<D: Domain>(log: Seq<D::Action>) -> D::Model
    decreases log.len(),
{
    if log.len() == 0 {
        D::init()
    } else {
        step::<D>(replay_log::<D>(log.drop_last()), log.last())
    }
}

pub open spec fn server_ok<D: Domain>(s: Server<D>) -> bool {
    &&& s.version == s.log.len()
    &&& s.present == replay_log::<D>(s.log)
    &&& D::inv(s.present)
}

pub open spec fn dispatch<D: AuthDomain>(
    s: Server<D>,
    client_version: nat,
    a: D::Action,
) -> (Server<D>, Reply<D::Model>) {
    if client_version != s.version {
        (s, Reply::Stale { version: s.version, model: s.present })
    } else if !D::accepts(s.present, a) {
        (s, Reply::Rejected { version: s.version, model: s.present })
    } else {
        let m2 = step::<D>(s.present, a);
        let s2 = Server { version: s.version + 1, present: m2, log: s.log.push(a) };
        (s2, Reply::Accepted { version: s2.version, model: m2 })
    }
}

} // verus!
