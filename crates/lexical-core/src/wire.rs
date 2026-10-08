//! Binary serialization through the [JetStream](https://jetstream.rs) wire format
//! (`jetstream_wireformat`), enabled by the `jetstream` feature.
//!
//! The document types derive `JetStreamWireFormat` themselves (see `node.rs`,
//! `state/mod.rs`, `selection.rs`); there is no second set of wire-only types. This module
//! holds what the derive cannot say on its own:
//!
//! * [`wide`]: field codecs for lengths past JetStream's `u16` (long text, many children,
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

/// Field codecs with `u32` lengths, for `#[jetstream(with(crate::wire::wide::...))]`.
/// JetStream's own `String`, `Vec` and `HashMap` use `u16` lengths (they come from a
/// protocol of small messages), which would cap a text node at 64 KiB and a document at
/// 65,535 nodes.
pub mod wide {
    use super::*;
    use std::collections::HashMap;
    use std::hash::Hash;

    /// How many elements to reserve up front when decoding: the length comes from the
    /// bytes, so it must not decide the allocation.
    const RESERVE: usize = 4096;

    fn too_long(what: &str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidInput, format!("{what} is too long"))
    }

    /// A `String` with a `u32` byte length.
    pub struct Text;

    impl Text {
        pub fn byte_size(s: &str) -> u32 {
            4u32.saturating_add(s.len() as u32)
        }

        pub fn encode<W: Write>(s: &str, writer: &mut W) -> io::Result<()> {
            let len = u32::try_from(s.len()).map_err(|_| too_long("text"))?;
            len.encode(writer)?;
            writer.write_all(s.as_bytes())
        }

        pub fn decode<R: Read>(reader: &mut R) -> io::Result<String> {
            let len = u32::decode(reader)? as u64;
            let mut bytes = Vec::new();
            let read = reader.take(len).read_to_end(&mut bytes)? as u64;
            if read != len {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        }
    }

    /// A `Vec` with a `u32` element count.
    pub struct Seq;

    impl Seq {
        pub fn byte_size<T: WireFormat>(v: &[T]) -> u32 {
            v.iter().fold(4u32, |n, x| n.saturating_add(x.byte_size()))
        }

        pub fn encode<T: WireFormat, W: Write>(v: &[T], writer: &mut W) -> io::Result<()> {
            u32::try_from(v.len()).map_err(|_| too_long("sequence"))?.encode(writer)?;
            v.iter().try_for_each(|x| x.encode(writer))
        }

        pub fn decode<T: WireFormat, R: Read>(reader: &mut R) -> io::Result<Vec<T>> {
            let len = u32::decode(reader)? as usize;
            let mut out = Vec::with_capacity(len.min(RESERVE));
            for _ in 0..len {
                out.push(T::decode(reader)?);
            }
            Ok(out)
        }
    }

    /// A `HashMap` with a `u32` entry count, written in key order so equal maps always
    /// produce equal bytes.
    pub struct Map;

    impl Map {
        pub fn byte_size<K: WireFormat, V: WireFormat>(m: &HashMap<K, V>) -> u32 {
            m.iter().fold(4u32, |n, (k, v)| n.saturating_add(k.byte_size()).saturating_add(v.byte_size()))
        }

        pub fn encode<K, V, W>(m: &HashMap<K, V>, writer: &mut W) -> io::Result<()>
        where
            K: WireFormat + Ord,
            V: WireFormat,
            W: Write,
        {
            u32::try_from(m.len()).map_err(|_| too_long("map"))?.encode(writer)?;
            let mut entries: Vec<_> = m.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            for (k, v) in entries {
                k.encode(writer)?;
                v.encode(writer)?;
            }
            Ok(())
        }

        pub fn decode<K, V, R>(reader: &mut R) -> io::Result<HashMap<K, V>>
        where
            K: WireFormat + Eq + Hash,
            V: WireFormat,
            R: Read,
        {
            let len = u32::decode(reader)? as usize;
            let mut out = HashMap::with_capacity(len.min(RESERVE));
            for _ in 0..len {
                let k = K::decode(reader)?;
                let v = V::decode(reader)?;
                if out.insert(k, v).is_some() {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "duplicate map key"));
                }
            }
            Ok(out)
        }
    }
}

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
            return Err(Error::Invalid("next node key is not above every node key".into()));
        }
        if let Some((k, n)) = self.nodes.iter().find(|(k, n)| **k != n.key) {
            return Err(Error::Invalid(format!("node stored under {k} says it is {}", n.key)));
        }
        self.check_invariants().map_err(Error::Invalid)?;
        self.validate_selection();
        Ok(())
    }
}
