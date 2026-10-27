//! The single fused artifact: morphotactics + unified trie, one file.

use super::Lexer;
use crate::codec::{self, Reader, Writer};
use crate::error::{art_err, Result};
use crate::morphotactics::Morphotactics;
use crate::subword::{WpTrie, ROOT_INIT};

pub const MAGIC: &[u8; 4] = b"MCFU";

impl Lexer {
    /// Serialize the compiled lexer (load it back with [`Self::from_bytes`]
    /// to skip compilation).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer::new();
        self.mt.write(&mut w);
        self.trie.write(&mut w);
        codec::wrap(MAGIC, &w.into_inner())
    }

    /// Load a lexer saved by [`Self::to_bytes`]. Every table is validated:
    /// a corrupt or foreign file is an `Err`, never a panic.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(codec::unwrap(MAGIC, data)?);
        let mt = Morphotactics::read(&mut r)?;
        let trie = WpTrie::read(&mut r, mt.entries.len())?;
        r.finish()?;
        // Each entry must sit exactly at the node spelling its surface.
        if trie.payload_total() != mt.entries.len() {
            return art_err("payload count does not match entry table");
        }
        for (id, e) in mt.entries.iter().enumerate() {
            let mut u = ROOT_INIT;
            for c in e.surface.chars() {
                match trie.goto(u, c) {
                    Some(v) => u = v,
                    None => return art_err("entry surface missing from trie"),
                }
            }
            if !trie.payload(u).contains(&(id as u32)) {
                return art_err("entry not attached at its surface node");
            }
        }
        Ok(Lexer { mt, trie })
    }
}
