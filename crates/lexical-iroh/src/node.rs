use crate::service::{Handshake, Replicate, ReplicateChannel, ReplicateService};
use crate::{Error, Result};
use crate::transport::{IrohServer, IrohTransport};
use iroh::endpoint::{presets, Endpoint};
use iroh::protocol::Router;
use iroh::EndpointAddr;
pub use iroh::EndpointId;
pub use iroh_tickets::endpoint::EndpointTicket as Ticket;
use jetstream::prelude::{Context, Protocol};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, oneshot};

/// What the transport needs from the thread that owns the document.
#[derive(Debug)]
pub enum Event {
    /// The endpoint reached its relay: this ticket is now dialable from other networks.
    /// ([`Node::ticket`] works earlier but may only carry local addresses.)
    Ready(Ticket),
    /// A remote update to import (`Collab::receive`). Duplicates are harmless.
    Update(Vec<u8>),
    /// Answer with this replica's version (`Collab::version`).
    Version(oneshot::Sender<Vec<u8>>),
    /// Answer with what a peer at `version` is missing (`Collab::updates_since`).
    Missing { version: Vec<u8>, reply: oneshot::Sender<Vec<u8>> },
    /// A peer connected (or reconnected) and has been brought up to date.
    PeerUp(EndpointId),
    /// A connection dropped; it is retried with backoff.
    PeerDown(EndpointId),
}

struct Inner {
    endpoint: Endpoint,
    events: mpsc::UnboundedSender<Event>,
    peers: Mutex<HashMap<EndpointId, mpsc::UnboundedSender<Vec<u8>>>>,
    handle: tokio::runtime::Handle,
}

/// One participant: serves the sync service and dials the peers it was told about.
pub struct Node {
    inner: Arc<Inner>,
    router: Router,
    rt: Option<Runtime>,
}

#[derive(Clone)]
struct Handler(Arc<Inner>);

impl std::fmt::Debug for Handler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Handler")
    }
}

const ALPN: &[u8] = <ReplicateChannel as Protocol>::NAME.as_bytes();
const BACKOFF_START: Duration = Duration::from_millis(500);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

impl Node {
    /// Bind an iroh endpoint and start serving. The receiver yields [`Event`]s; poll it on
    /// the thread that owns the document.
    pub fn spawn() -> Result<(Node, mpsc::UnboundedReceiver<Event>)> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("lexical-iroh")
            .build()
            .map_err(|e| Error::Endpoint(e.to_string()))?;
        let (events, rx) = mpsc::unbounded_channel();
        let (inner, router) = rt.block_on(async {
            let endpoint = Endpoint::bind(presets::N0).await.map_err(|e| Error::Endpoint(e.to_string()))?;
            let inner = Arc::new(Inner { endpoint: endpoint.clone(), events, peers: Mutex::default(), handle: tokio::runtime::Handle::current() });
            let router = Router::builder(endpoint)
                .accept(
                    ALPN, IrohServer::new(ReplicateService { inner: Handler(inner.clone()) }),
                )
                .spawn();
            let announce = inner.clone();
            tokio::spawn(async move {
                announce.endpoint.online().await;
                let _ = announce.events.send(Event::Ready(announce.ticket()));
            });
            Ok::<_, Error>((inner, router))
        })?;
        Ok((Node { inner, router, rt: Some(rt) }, rx))
    }

    /// This node's id, which is its public key.
    pub fn id(&self) -> EndpointId {
        self.inner.endpoint.id()
    }

    /// What another node needs to dial this one.
    pub fn ticket(&self) -> Ticket {
        let _guard = self.inner.handle.enter();
        self.inner.ticket()
    }

    /// Dial the node behind `ticket` and keep the connection up. Returns at once; progress
    /// arrives as [`Event::PeerUp`] / [`Event::PeerDown`]. The other side dials back, so
    /// only one of two nodes has to learn the other's ticket.
    pub fn connect(&self, ticket: Ticket) {
        let addr = ticket.endpoint_addr().clone();
        let _guard = self.inner.handle.enter();
        if addr.id != self.id() {
            self.inner.ensure_peer(addr);
        }
    }

    /// Send a local update (`Collab::drain_updates`) to every connected peer. Updates for a
    /// peer that is down wait for it; its next `hello` catches it up regardless.
    pub fn broadcast(&self, update: Vec<u8>) {
        for tx in self.inner.peers.lock().unwrap().values() {
            let _ = tx.send(update.clone());
        }
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        if let Some(rt) = self.rt.take() {
            let router = self.router.clone();
            rt.block_on(async move {
                let _ = tokio::time::timeout(Duration::from_secs(1), router.shutdown()).await;
            });
            rt.shutdown_background();
        }
    }
}

impl Inner {
    fn ticket(&self) -> Ticket {
        Ticket::new(self.endpoint.addr())
    }

    /// Start the connection task for `addr` unless there is one. Returns whether it started.
    fn ensure_peer(self: &Arc<Self>, addr: EndpointAddr) -> bool {
        let (tx, rx) = mpsc::unbounded_channel();
        {
            let mut peers = self.peers.lock().unwrap();
            if peers.contains_key(&addr.id) {
                return false;
            }
            peers.insert(addr.id, tx);
        }
        self.handle.spawn(run_peer(self.clone(), addr, rx));
        true
    }

    /// Ask the document's owner something; `None` when it has gone away.
    async fn ask<T>(&self, event: impl FnOnce(oneshot::Sender<T>) -> Event) -> Option<T> {
        let (tx, rx) = oneshot::channel();
        self.events.send(event(tx)).ok()?;
        rx.await.ok()
    }
}

type RpcResult<T> = std::result::Result<T, String>;

/// Hold one connection to `addr` open, reconnecting with backoff, until the node stops.
async fn run_peer(inner: Arc<Inner>, addr: EndpointAddr, mut outbox: mpsc::UnboundedReceiver<Vec<u8>>) {
    let id = addr.id;
    let mut backoff = BACKOFF_START;
    loop {
        match session(&inner, &addr, &mut outbox, &mut backoff).await {
            Ok(()) => return,
            Err(e) => tracing::debug!(peer = %id.fmt_short(), "sync session ended: {e}"),
        }
        if inner.events.send(Event::PeerDown(id)).is_err() {
            return;
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(BACKOFF_MAX);
    }
}

/// One connection: introduce ourselves, trade what each side lacks, then forward updates.
/// `Ok` only when the document's owner is gone.
async fn session(
    inner: &Arc<Inner>,
    addr: &EndpointAddr,
    outbox: &mut mpsc::UnboundedReceiver<Vec<u8>>,
    backoff: &mut Duration,
) -> RpcResult<()> {
    let conn = inner
        .endpoint
        .connect(addr.clone(), ALPN)
        .await
        .map_err(|e| e.to_string())?;
    let streams = conn.open_bi().await.map_err(|e| e.to_string())?;
    let channel = ReplicateChannel::new(16, Box::new(IrohTransport::<ReplicateChannel>::from(streams)));

    let Some(mine) = inner.ask(Event::Version).await else { return Ok(()) };
    let theirs = channel
        .hello(Context::default(), inner.ticket().to_string(), mine)
        .await
        .map_err(|e| e.to_string())?;
    if !theirs.updates.is_empty() && inner.events.send(Event::Update(theirs.updates)).is_err() {
        return Ok(());
    }
    let Some(missing) = inner.ask(|reply| Event::Missing { version: theirs.version, reply }).await else {
        return Ok(());
    };
    if !missing.is_empty() {
        channel.push(Context::default(), missing).await.map_err(|e| e.to_string())?;
    }
    *backoff = BACKOFF_START;
    if inner.events.send(Event::PeerUp(addr.id)).is_err() {
        return Ok(());
    }

    loop {
        tokio::select! {
            update = outbox.recv() => {
                let Some(update) = update else { return Ok(()) };
                channel.push(Context::default(), update).await.map_err(|e| e.to_string())?;
            }
            reason = conn.closed() => return Err(reason.to_string()),
        }
    }
}

impl Replicate for Handler {
    async fn hello(&self, _ctx: Context, ticket: String, version: Vec<u8>) -> jetstream::prelude::Result<Handshake> {
        let bad = |what: String| jetstream::prelude::Error::from(std::io::Error::other(what));
        let ticket: Ticket = ticket.parse().map_err(|e| bad(format!("invalid ticket: {e}")))?;
        let addr = ticket.endpoint_addr().clone();
        if addr.id != self.0.endpoint.id() {
            // A stranger: dial them back so our updates reach them too.
            self.0.ensure_peer(addr);
        }
        let gone = || bad("document closed".into());
        let mine = self.0.ask(Event::Version).await.ok_or_else(gone)?;
        let updates = self.0.ask(|reply| Event::Missing { version, reply }).await.ok_or_else(gone)?;
        Ok(Handshake { version: mine, updates })
    }

    async fn push(&self, _ctx: Context, update: Vec<u8>) -> jetstream::prelude::Result<String> {
        let _ = self.0.events.send(Event::Update(update));
        Ok(String::new())
    }
}
