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
//!         `inv(server.present)` holds,
//!   (AK2) a rejected request (stale or domain-invalid) leaves the server unchanged,
//!   (AK3) the version is exactly the length of the applied log and grows by one
//!         per accepted request, never otherwise,
//!   (AK4) **replay**: the server state is always the fold of the applied log from
//!         `init()` (state = replay of history; the log is a complete record),
//!   (AK5) completeness: a fresh (non-stale) request the domain accepts is applied,
//!   (AK6) clients: optimistic local actions keep the client model valid, and
//!         re-basing pending actions onto a fresh server state keeps it valid.
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

pub proof fn init_server_ok<D: Domain>()
    ensures server_ok::<D>(init_server::<D>()),
{
    D::init_satisfies_inv();
}

/// (AK1, AK3, AK4) One request preserves everything.
pub proof fn dispatch_preserves<D: AuthDomain>(s: Server<D>, v: nat, a: D::Action)
    requires server_ok::<D>(s),
    ensures server_ok::<D>(dispatch::<D>(s, v, a).0),
{
    let (s2, _) = dispatch::<D>(s, v, a);
    if v == s.version && D::accepts(s.present, a) {
        D::step_preserves_inv(s.present, a);
        assert(s2.log.drop_last() =~= s.log);
    }
}

/// (AK2) Rejections change nothing.
pub proof fn rejection_is_a_noop<D: AuthDomain>(s: Server<D>, v: nat, a: D::Action)
    ensures ({
        let (s2, r) = dispatch::<D>(s, v, a);
        !(r is Accepted) ==> s2 == s
    }),
{
}

/// (AK3) The version moves by exactly one on acceptance and not at all otherwise.
pub proof fn version_is_monotone<D: AuthDomain>(s: Server<D>, v: nat, a: D::Action)
    ensures ({
        let (s2, r) = dispatch::<D>(s, v, a);
        &&& (r is Accepted ==> s2.version == s.version + 1)
        &&& (!(r is Accepted) ==> s2.version == s.version)
    }),
{
}

/// (AK5) A fresh request that the domain accepts is applied.
pub proof fn fresh_valid_request_is_accepted<D: AuthDomain>(s: Server<D>, a: D::Action)
    requires D::accepts(s.present, a),
    ensures dispatch::<D>(s, s.version, a).1 is Accepted,
{
}

/// Requests arrive in any order from any client.
pub struct Request<A> {
    pub base_version: nat,
    pub action: A,
}

pub open spec fn serve<D: AuthDomain>(reqs: Seq<Request<D::Action>>) -> Server<D>
    decreases reqs.len(),
{
    if reqs.len() == 0 {
        init_server::<D>()
    } else {
        let s = serve::<D>(reqs.drop_last());
        dispatch::<D>(s, reqs.last().base_version, reqs.last().action).0
    }
}

/// (AK1 + AK4 over arbitrary traces) Safety does not depend on client behaviour.
pub proof fn serve_preserves<D: AuthDomain>(reqs: Seq<Request<D::Action>>)
    ensures server_ok::<D>(serve::<D>(reqs)),
    decreases reqs.len(),
{
    if reqs.len() == 0 {
        init_server_ok::<D>();
    } else {
        serve_preserves::<D>(reqs.drop_last());
        dispatch_preserves::<D>(
            serve::<D>(reqs.drop_last()),
            reqs.last().base_version,
            reqs.last().action,
        );
    }
}

// ---------------------------------------------------------------------- clients

/// Optimistic client: last known server version/model plus not-yet-acknowledged actions.
pub struct Client<D: Domain> {
    pub version: nat,
    pub base: D::Model,
    pub pending: Seq<D::Action>,
}

/// Apply pending actions on top of a base model.
pub open spec fn reapply<D: Domain>(m: D::Model, pending: Seq<D::Action>) -> D::Model
    decreases pending.len(),
{
    if pending.len() == 0 {
        m
    } else {
        step::<D>(reapply::<D>(m, pending.drop_last()), pending.last())
    }
}

/// What the user sees: the base with the pending actions applied optimistically.
pub open spec fn client_model<D: Domain>(c: Client<D>) -> D::Model {
    reapply::<D>(c.base, c.pending)
}

pub proof fn reapply_preserves<D: Domain>(m: D::Model, pending: Seq<D::Action>)
    requires D::inv(m),
    ensures D::inv(reapply::<D>(m, pending)),
    decreases pending.len(),
{
    if pending.len() > 0 {
        reapply_preserves::<D>(m, pending.drop_last());
        D::step_preserves_inv(reapply::<D>(m, pending.drop_last()), pending.last());
    }
}

pub open spec fn client_local<D: Domain>(c: Client<D>, a: D::Action) -> Client<D> {
    Client { version: c.version, base: c.base, pending: c.pending.push(a) }
}

/// Take the server's state and re-apply what is still pending (rebase).
pub open spec fn client_sync<D: Domain>(c: Client<D>, s: Server<D>) -> Client<D> {
    Client { version: s.version, base: s.present, pending: c.pending }
}

/// The server acknowledged the oldest pending action.
pub open spec fn client_ack<D: Domain>(c: Client<D>, version: nat, model: D::Model) -> Client<D> {
    Client {
        version,
        base: model,
        pending: if c.pending.len() > 0 { c.pending.skip(1) } else { c.pending },
    }
}

/// (AK6) What the user sees is always valid, after local edits and after syncing.
pub proof fn client_local_preserves<D: Domain>(c: Client<D>, a: D::Action)
    requires D::inv(c.base),
    ensures D::inv(client_model::<D>(client_local::<D>(c, a))),
{
    reapply_preserves::<D>(c.base, c.pending.push(a));
}

pub proof fn client_sync_preserves<D: Domain>(c: Client<D>, s: Server<D>)
    requires server_ok::<D>(s),
    ensures
        D::inv(client_sync::<D>(c, s).base),
        D::inv(client_model::<D>(client_sync::<D>(c, s))),
{
    reapply_preserves::<D>(s.present, c.pending);
}

pub proof fn client_ack_preserves<D: Domain>(c: Client<D>, version: nat, model: D::Model)
    requires D::inv(model),
    ensures D::inv(client_model::<D>(client_ack::<D>(c, version, model))),
{
    reapply_preserves::<D>(model, client_ack::<D>(c, version, model).pending);
}

} // verus!
