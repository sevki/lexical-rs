//! (AK6) Optimistic clients: pending local actions on top of the last known server state.

use super::*;
use vstd::prelude::*;

verus! {

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

/// What the user sees is always valid, after local edits and after syncing.
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
