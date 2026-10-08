//! Binary serialization through the [JetStream](https://jetstream.rs) wire format
//! (`jetstream_wireformat`), enabled by the `jetstream` feature.
//!
//! The document types derive `JetStreamWireFormat` themselves (see `node.rs`,
//! `state/mod.rs`, `selection.rs`); there is no second set of wire-only types. This module
//! holds what the derive cannot say on its own:
//!
//! * [`wide`] (in `wire/wide.rs`): field codecs for lengths past JetStream's `u16` (long text, many children,
//!   many nodes), used with `#[jetstream(with(...))]`;
//! * `WireFormat for TextFormat`, a `bitflags` type;
//! * checking decoded bytes: [`EditorState::from_wire_bytes`] and [`EditorState::check_wire`].

use crate::error::{Error, Result};
use crate::format::TextFormat;
use crate::state::EditorState;
use jetstream_wireformat::WireFormat;
use std::io::{self, Read, Write};

/// First byte of [`EditorState::to_wire_bytes`]. Bumped when the layout changes.
pub const WIRE_VERSION: u8 = 2;

impl WireFormat for TextFormat {
    fn byte_size(&self) -> u32 {
        4
    }

    fn encode<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        self.bits().encode(writer)
    }

    fn decode<R: Read>(reader: &mut R) -> io::Result<Self> {
        Ok(TextFormat::from_bits_truncate(u32::decode(reader)?))
    }
}

pub mod wide;

fn invalid(msg: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

impl EditorState {
    /// The document as `[WIRE_VERSION]` followed by its JetStream encoding. The bytes are
    /// canonical: equal documents give equal bytes.
    pub fn to_wire_bytes(&self) -> io::Result<Vec<u8>> {
        let mut out = Vec::with_capacity(1 + self.byte_size() as usize);
        out.push(WIRE_VERSION);
        self.encode(&mut out)?;
        Ok(out)
    }

    /// Decode bytes from [`EditorState::to_wire_bytes`] and check that they describe a valid
    /// document. Anything that does not is an error, never a panic later.
    pub fn from_wire_bytes(bytes: &[u8]) -> io::Result<EditorState> {
        let (&version, mut rest) = bytes.split_first().ok_or_else(|| invalid("empty input"))?;
        if version != WIRE_VERSION {
            return Err(invalid(format!("unsupported wire version {version}")));
        }
        let mut state = EditorState::decode(&mut rest)?;
        if !rest.is_empty() {
            return Err(invalid("trailing bytes after the document"));
        }
        state.check_wire().map_err(invalid)?;
        Ok(state)
    }

    /// Check a document that came out of `WireFormat::decode` (for instance as a JetStream
    /// message): the arena must be a well-formed tree and satisfy the editor's invariants,
    /// and a selection pointing at nothing is dropped. Plain decoding checks nothing about
    /// how nodes relate to each other, so call this before using an untrusted document.
    pub fn check_wire(&mut self) -> Result<()> {
        let max = self.nodes.keys().map(|k| k.0).max().unwrap_or(0);
        if self.next_key() <= max {
            return Err(Error::Invalid(
                "next node key is not above every node key".into(),
            ));
        }
        if let Some((k, n)) = self.nodes.iter().find(|(k, n)| **k != n.key) {
            return Err(Error::Invalid(format!(
                "node stored under {k} says it is {}",
                n.key
            )));
        }
        self.check_invariants().map_err(Error::Invalid)?;
        self.validate_selection();
        Ok(())
    }
}
