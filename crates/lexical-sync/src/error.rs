use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncError {
    /// The CRDT rejected an operation or a message.
    Crdt(String),
    /// The peer id collides with the reserved bootstrap peer.
    ReservedPeer,
    /// A presence or version message could not be decoded.
    BadMessage(String),
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncError::Crdt(m) => write!(f, "crdt error: {m}"),
            SyncError::ReservedPeer => write!(f, "peer id is reserved for the shared bootstrap"),
            SyncError::BadMessage(m) => write!(f, "bad message: {m}"),
        }
    }
}

impl std::error::Error for SyncError {}

pub type Result<T> = std::result::Result<T, SyncError>;

pub(crate) fn crdt(e: impl fmt::Display) -> SyncError {
    SyncError::Crdt(e.to_string())
}
