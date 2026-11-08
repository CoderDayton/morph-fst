//! LinMaxMatch as an incremental stepper, so the fused lexer can run it in
//! lock-step with morphological walkers over a single scan.
//!
//! Invariant (from the closure property): for every non-root node `v`,
//! `str(v) = concat(pops(v)) ++ str(fail(v))`, and `fail` strictly decreases
//! depth. Hence each failure transition is paid for by an earlier `goto`, so
//! total steps ≤ 2·|word|.

use super::trie::{WpTrie, ROOT_CONT, ROOT_INIT};
use crate::output::SubToken;

#[derive(Clone, Copy, Debug)]
pub struct LinMax {
    u: u32,
}

impl Default for LinMax {
    fn default() -> Self {
        Self::new()
    }
}

impl LinMax {
    pub fn new() -> Self {
        LinMax { u: ROOT_INIT }
    }

    #[inline]
    fn at_root(&self) -> bool {
        self.u == ROOT_INIT || self.u == ROOT_CONT
    }

    #[inline]
    pub fn step(&mut self, t: &WpTrie, c: char, out: &mut Vec<SubToken>, steps: &mut u64) {
        loop {
            if let Some(v) = t.goto(self.u, c) {
                *steps += 1;
                self.u = v;
                return;
            }
            if self.at_root() {
                // `c` is outside the vocabulary alphabet: byte fallback.
                let mut buf = [0u8; 4];
                out.extend(c.encode_utf8(&mut buf).bytes().map(SubToken::Byte));
                self.u = ROOT_CONT;
                return;
            }
            *steps += 1;
            out.extend(t.pops(self.u).iter().map(|&p| t.piece_token(p)));
            self.u = t.fail(self.u);
        }
    }

    #[inline]
    pub fn finish(&mut self, t: &WpTrie, out: &mut Vec<SubToken>, steps: &mut u64) {
        while !self.at_root() {
            *steps += 1;
            out.extend(t.pops(self.u).iter().map(|&p| t.piece_token(p)));
            self.u = t.fail(self.u);
        }
        self.u = ROOT_INIT;
    }
}
