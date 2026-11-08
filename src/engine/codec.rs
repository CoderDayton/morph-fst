//! Minimal little-endian binary codec used by every artifact.
//!
//! Layout of a wrapped artifact:
//! `magic[4] | version u32 | payload_len u64 | fnv1a64(payload) u64 | payload`
//!
//! The reader never trusts a length field: every read is bounds-checked and
//! array lengths are checked against the remaining bytes *before* allocating.
//! It reads untrusted input, so indexing and unchecked arithmetic are denied.

#![deny(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use crate::error::{art_err, Result};

pub const FORMAT_VERSION: u32 = 8;

#[derive(Default)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn bytes(&mut self, b: &[u8]) {
        self.u32(b.len() as u32);
        self.buf.extend_from_slice(b);
    }
    pub fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
    pub fn u32s(&mut self, v: &[u32]) {
        self.u32(v.len() as u32);
        for &x in v {
            self.u32(x);
        }
    }
    pub fn u64s(&mut self, v: &[u64]) {
        self.u32(v.len() as u32);
        for &x in v {
            self.u64(x);
        }
    }
    pub fn into_inner(self) -> Vec<u8> {
        self.buf
    }
}

/// Reads from the front of `rest`; `p` is the offset consumed so far.
pub struct Reader<'a> {
    rest: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    pub fn new(d: &'a [u8]) -> Self {
        Reader { rest: d, p: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let Some((s, rest)) = self.rest.split_at_checked(n) else {
            return art_err(format!("unexpected end of data at offset {} (need {n} bytes)", self.p));
        };
        self.rest = rest;
        self.p = self.p.saturating_add(n);
        Ok(s)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        match self.take(N)?.first_chunk::<N>() {
            Some(a) => Ok(*a),
            None => art_err("short read"),
        }
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(u8::from_le_bytes(self.array()?))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array()?))
    }
    pub fn bytes(&mut self) -> Result<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }
    pub fn string(&mut self) -> Result<String> {
        let b = self.bytes()?;
        match std::str::from_utf8(b) {
            Ok(s) => Ok(s.to_owned()),
            Err(_) => art_err("invalid UTF-8 string"),
        }
    }
    pub fn u32s(&mut self) -> Result<Vec<u32>> {
        let n = self.u32()? as usize;
        let raw = self.take(n.checked_mul(4).ok_or_else(|| crate::Error::Artifact("length overflow".into()))?)?;
        Ok(raw.chunks_exact(4).filter_map(|c| c.first_chunk().map(|a| u32::from_le_bytes(*a))).collect())
    }
    pub fn u64s(&mut self) -> Result<Vec<u64>> {
        let n = self.u32()? as usize;
        let raw = self.take(n.checked_mul(8).ok_or_else(|| crate::Error::Artifact("length overflow".into()))?)?;
        Ok(raw.chunks_exact(8).filter_map(|c| c.first_chunk().map(|a| u64::from_le_bytes(*a))).collect())
    }
    /// Bounded element count for a list whose elements take at least
    /// `min_elem_bytes` each; rejects counts that cannot possibly fit.
    pub fn count(&mut self, min_elem_bytes: usize) -> Result<usize> {
        let n = self.u32()? as usize;
        if n.saturating_mul(min_elem_bytes.max(1)) > self.rest.len() {
            return art_err(format!("list length {n} exceeds remaining data"));
        }
        Ok(n)
    }
    pub fn finish(&self) -> Result<()> {
        if !self.rest.is_empty() {
            return art_err(format!("{} trailing bytes", self.rest.len()));
        }
        Ok(())
    }
}

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub fn wrap(magic: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len().saturating_add(24));
    out.extend_from_slice(magic);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(&fnv1a64(payload).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

pub fn unwrap<'a>(magic: &[u8; 4], data: &'a [u8]) -> Result<&'a [u8]> {
    let mut r = Reader::new(data);
    let m = r.take(4)?;
    if m != magic {
        return art_err(format!(
            "bad magic {:?}, expected {:?}",
            String::from_utf8_lossy(m),
            String::from_utf8_lossy(magic)
        ));
    }
    let v = r.u32()?;
    if v != FORMAT_VERSION {
        return art_err(format!("unsupported format version {v}"));
    }
    let len = r.u64()?;
    let sum = r.u64()?;
    let payload = r.rest;
    if payload.len() as u64 != len {
        return art_err(format!("payload length {} != header length {len}", payload.len()));
    }
    if fnv1a64(payload) != sum {
        return art_err("checksum mismatch (artifact corrupted)");
    }
    Ok(payload)
}
