//! Field codecs with `u32` lengths, for `#[jetstream(with(crate::wire::wide::...))]`.
//! JetStream's own `String`, `Vec` and `HashMap` use `u16` lengths (they come from a
//! protocol of small messages), which would cap a text node at 64 KiB and a document at
//! 65,535 nodes.

use jetstream_wireformat::{Data, WireFormat};
use std::collections::HashMap;
use std::hash::Hash;
use std::io::{self, Read, Write};

/// How many elements to reserve up front when decoding: the length comes from the
/// bytes, so it must not decide the allocation.
const RESERVE: usize = 4096;

fn too_long(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, format!("{what} is too long"))
}

/// A `String` with a `u32` byte length: the UTF-8 bytes as a JetStream [`Data`], which
/// also caps what a decoder will accept (32 MiB).
pub struct Text;

impl Text {
    pub fn byte_size(s: &str) -> u32 {
        4u32.saturating_add(s.len() as u32)
    }

    pub fn encode<W: Write>(s: &str, writer: &mut W) -> io::Result<()> {
        Data(s.as_bytes().to_vec()).encode(writer)
    }

    pub fn decode<R: Read>(reader: &mut R) -> io::Result<String> {
        String::from_utf8(Data::decode(reader)?.0)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

/// A `Vec` with a `u32` element count.
pub struct Seq;

impl Seq {
    pub fn byte_size<T: WireFormat>(v: &[T]) -> u32 {
        v.iter().fold(4u32, |n, x| n.saturating_add(x.byte_size()))
    }

    pub fn encode<T: WireFormat, W: Write>(v: &[T], writer: &mut W) -> io::Result<()> {
        u32::try_from(v.len())
            .map_err(|_| too_long("sequence"))?
            .encode(writer)?;
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
        m.iter().fold(4u32, |n, (k, v)| {
            n.saturating_add(k.byte_size())
                .saturating_add(v.byte_size())
        })
    }

    pub fn encode<K, V, W>(m: &HashMap<K, V>, writer: &mut W) -> io::Result<()>
    where
        K: WireFormat + Ord,
        V: WireFormat,
        W: Write,
    {
        u32::try_from(m.len())
            .map_err(|_| too_long("map"))?
            .encode(writer)?;
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
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "duplicate map key",
                ));
            }
        }
        Ok(out)
    }
}
