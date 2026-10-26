//! Deterministic WordPiece fallback.
//!
//! * [`trie`]: a char trie with two roots (`ROOT_INIT` for word-initial
//!   pieces, `ROOT_CONT` for `##` pieces), optional per-node morpheme
//!   payloads (used by the fused lexer), and Aho–Corasick-style failure links
//!   + failure pops computed in one BFS.
//! * [`linmax`]: LinMaxMatch (Song et al., EMNLP 2021) — greedy
//!   longest-match-first WordPiece in O(n), extended with per-character byte
//!   fallback so it is total: it never emits `[UNK]` and never fails.
//! * [`mod@reference`]: the naive O(n·m) greedy MaxMatch used as a test oracle.

pub mod linmax;
pub mod reference;
pub mod trie;

use std::collections::BTreeSet;

pub use linmax::LinMax;
pub use trie::{PieceDef, TrieBuilder, WpTrie, NONE, ROOT_CONT, ROOT_INIT};

/// Sorted, deduplicated piece set closed under single characters: every
/// character of every piece becomes both an initial and a `##` piece. This
/// closure is what makes every failure chain end at a root (see README).
pub fn closed_piece_set<'a, I: IntoIterator<Item = (&'a str, bool)>>(pieces: I) -> BTreeSet<(bool, String)> {
    let mut set: BTreeSet<(bool, String)> = BTreeSet::new();
    let mut alphabet: BTreeSet<char> = BTreeSet::new();
    for (text, cont) in pieces {
        if text.is_empty() {
            continue;
        }
        alphabet.extend(text.chars());
        if !set.contains(&(cont, text.to_string())) {
            set.insert((cont, text.to_string()));
        }
    }
    for c in alphabet {
        let s = c.to_string();
        set.insert((false, s.clone()));
        set.insert((true, s));
    }
    set
}

/// A standalone WordPiece model (no morpheme payloads): pieces → trie →
/// failure links.
pub fn build_model(pieces: &[PieceDef]) -> WpTrie {
    let set = closed_piece_set(pieces.iter().map(|p| (p.text.as_str(), p.cont)));
    let mut tb = TrieBuilder::new();
    for (cont, text) in &set {
        tb.add_piece(text, *cont);
    }
    tb.finish()
}
