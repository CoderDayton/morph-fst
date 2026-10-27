//! Strict mode: one left-to-right scan that runs the morphological walkers
//! and LinMaxMatch in lock-step over the same trie.
//!
//! * A walker is spawned at every position the lattice can reach; it follows
//!   trie edges from `ROOT_INIT` and emits an arc for each payload it meets.
//!   Walkers are class-independent; morphotactics filter arcs in the lattice.
//! * Because all walkers advance together, every arc ending at `p` is known
//!   before the scan reads the character at `p` — exactly the builder's
//!   contract.
//! * LinMaxMatch's output is buffered and used only if the lattice rejects.

use super::Lexer;
use crate::lattice::LatticeBuilder;
use crate::output::WordOutput;
use crate::subword::{LinMax, ROOT_INIT};
use crate::ANALYSES_CAP;

impl Lexer {
    #[doc(hidden)]
    pub fn lattice(&self, word: &str) -> Option<crate::lattice::Lattice> {
        let (lat, _) = self.scan(word, false);
        lat
    }

    /// The fused scan. Returns the lattice (if morphology ran) and, when
    /// `with_pieces`, the LinMaxMatch tokens.
    pub(crate) fn scan(&self, word: &str, with_pieces: bool) -> (Option<crate::lattice::Lattice>, Vec<crate::output::SubToken>) {
        let morph = !word.is_empty() && word.chars().count() <= self.mt.max_morph_chars as usize;
        let mut lb = morph.then(|| LatticeBuilder::new(&self.mt, word));
        let mut walkers: Vec<(u32, u32)> = Vec::with_capacity(word.len());
        let mut arcs: Vec<(u32, u32)> = Vec::with_capacity(16);
        let mut lm = LinMax::new();
        let mut pieces = Vec::new();
        let mut steps = 0u64;
        for (pos, c) in word.char_indices() {
            if let Some(lb) = lb.as_mut() {
                if lb.active(pos) {
                    walkers.push((pos as u32, ROOT_INIT));
                }
                let end = pos + c.len_utf8();
                arcs.clear();
                walkers.retain_mut(|w| match self.trie.goto(w.1, c) {
                    Some(v) => {
                        w.1 = v;
                        arcs.extend(self.trie.payload(v).iter().map(|&e| (w.0, e)));
                        true
                    }
                    None => false,
                });
                for &(s, e) in &arcs {
                    lb.add_arc(s as usize, end, e);
                }
            }
            if with_pieces {
                lm.step(&self.trie, c, &mut pieces, &mut steps);
            }
        }
        if with_pieces {
            lm.finish(&self.trie, &mut pieces, &mut steps);
        }
        (lb.map(|b| b.finish()), pieces)
    }

    /// All analyses of a normalized word (up to `cap`, best first), or
    /// `None` if the grammar rejects it. No fallback.
    #[doc(hidden)]
    pub fn analyze(&self, word: &str, cap: usize) -> Option<WordOutput> {
        let lat = self.lattice(word)?;
        let (best, all) = lat.decode(&self.mt, cap)?;
        Some(WordOutput::Analyzed { best, all })
    }

    /// [`Mode::Strict`](super::Mode::Strict) lexing of a normalized word.
    pub(crate) fn lex_strict(&self, word: &str) -> WordOutput {
        let (lat, pieces) = self.scan(word, true);
        if let Some(lat) = lat {
            if let Some((best, all)) = lat.decode(&self.mt, ANALYSES_CAP) {
                return WordOutput::Analyzed { best, all };
            }
        }
        WordOutput::Fallback(pieces)
    }
}
