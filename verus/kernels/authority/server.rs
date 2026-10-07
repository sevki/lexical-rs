//! (AK1-AK5) What the server guarantees, whatever clients send.

use super::*;
use vstd::prelude::*;

verus! {

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

} // verus!
