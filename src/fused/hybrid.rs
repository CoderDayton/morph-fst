//! Hybrid mode: when the strict parse fails, look for known affixes around
//! **one** unknown stem of an open class, with lexicographic cost
//! `(unknown chars, morpheme count)` — a tropical-style lexicographic
//! semiring. Any full parse has cost `(0, k)`, which beats every path with an
//! unknown stem, so hybrid ≡ strict whenever strict succeeds.
//!
//! Lattice nodes are `(pos, layer, state)`; layer 0 = before the unknown
//! stem, layer 1 = after it. The wildcard arc goes from an open-class state
//! in layer 0 to that class's continuation states in layer 1.

use super::FusedLexer;
use crate::output::{Morph, Segment, WordOutput};
use crate::subword::ROOT_INIT;

#[derive(Clone, Copy)]
enum Back {
    Start,
    Morph { from: (u32, u8, u32), entry: u32 },
    Wild { from: (u32, u8, u32) },
}

/// (unknown chars, exceptions the unknown stem must carry, morphemes),
/// packed so that integer order is lexicographic order: 21 bits each,
/// far above `max_morph_chars` and any morpheme or exception count.
type Cost = u64;
const UNK: u64 = 1 << 42;
const EXC: u64 = 1 << 21;
const MORPH: u64 = 1;
/// Per position: small list of ((layer, state), (best cost, back-pointer)).
/// Positions hold a handful of labels, so a linear scan beats a map; the
/// list is sorted by key before a position is expanded, which keeps the
/// processing order (and thus tie-breaking) fully deterministic.
type Labels = Vec<Vec<((u8, u32), (Cost, Back))>>;

impl FusedLexer {
    /// [`Mode::Hybrid`](super::Mode::Hybrid) lexing of a normalized word.
    pub fn lex_hybrid(&self, word: &str) -> WordOutput {
        let strict = self.lex_strict(word);
        if !matches!(strict, WordOutput::Fallback(_)) {
            return strict;
        }
        if word.is_empty() || word.chars().count() > self.mt.max_morph_chars as usize {
            return strict;
        }
        self.hybrid_parse(word).unwrap_or(strict)
    }

    fn hybrid_parse(&self, word: &str) -> Option<WordOutput> {
        let mt = &self.mt;
        let n = word.len();
        let bounds: Vec<usize> = word.char_indices().map(|(i, _)| i).chain(std::iter::once(n)).collect();
        // `b` is always one of `bounds`; Err would give its insertion point.
        let char_idx = |b: usize| bounds.binary_search(&b).unwrap_or_else(|i| i);
        let mut reg_at = Vec::new();
        mt.track(word, &mut reg_at);
        let reg = |p: usize| reg_at.get(p).copied().unwrap_or(0) as u32;
        let mut rreg_at = Vec::new();
        mt.track_right(word, &mut rreg_at);
        let rreg = |p: usize| rreg_at.get(p).copied().unwrap_or(0) as u32;
        let mut multi: Vec<u32> = Vec::new();
        let mut wild: Vec<(u32, u32)> = Vec::new();
        // Arcs from every boundary (stems after an unknown stem need them).
        let mut arcs_at: Vec<Vec<(u32, u32)>> = vec![Vec::new(); n + 1];
        for &b in &bounds[..bounds.len() - 1] {
            let mut u = ROOT_INIT;
            for (off, c) in word[b..].char_indices() {
                match self.trie.goto(u, c) {
                    Some(v) => u = v,
                    None => break,
                }
                let end = b + off + c.len_utf8();
                arcs_at[b].extend(self.trie.payload(u).iter().map(|&e| (end as u32, e)));
            }
        }
        let mut lab: Labels = vec![Vec::new(); n + 1];
        for &s in mt.start_states() {
            lab[0].push(((0, s), (0, Back::Start)));
        }
        let relax = |lab: &mut Labels, pos: usize, key: (u8, u32), cost: Cost, back: Back| {
            match lab[pos].iter_mut().find(|(k, _)| *k == key) {
                Some((_, (c, b))) => {
                    if cost < *c {
                        *c = cost;
                        *b = back;
                    }
                }
                None => lab[pos].push((key, (cost, back))),
            }
        };
        let mut here: Vec<((u8, u32), Cost)> = Vec::new();
        for &p in &bounds[..bounds.len() - 1] {
            lab[p].sort_unstable_by_key(|(k, _)| *k);
            here.clear();
            here.extend(lab[p].iter().map(|(k, (c, _))| (*k, *c)));
            let rp = reg(p);
            for &((layer, s), cost) in &here {
                for &(end, e) in &arcs_at[p] {
                    multi.clear();
                    if mt.needs_multi() {
                        mt.step_multi(s, e, rp, rreg(end as usize), &mut multi);
                    } else if let Some(t) = mt.step(s, e, rp, rreg(end as usize)) {
                        multi.push(t);
                    }
                    for &t in &multi {
                        for t2 in mt.closure(t) {
                            let back = Back::Morph { from: (p as u32, layer, s), entry: e };
                            relax(&mut lab, end as usize, (layer, t2), cost + MORPH, back);
                        }
                    }
                }
                if layer == 0 {
                    let ci = char_idx(p);
                    let per_span = mt.wildcard_reads_span();
                    if !per_span {
                        wild.clear();
                        mt.wildcard_into(s, "", &mut wild);
                        if wild.is_empty() {
                            continue;
                        }
                    }
                    for (k, &end) in bounds[ci + 1..].iter().enumerate() {
                        let wc = (k + 1) as u32;
                        if per_span {
                            wild.clear();
                            mt.wildcard_into(s, &word[p..end], &mut wild);
                        }
                        for &(t, exc) in &wild {
                            let back = Back::Wild { from: (p as u32, 0, s) };
                            relax(&mut lab, end, (1, t), cost + wc as u64 * UNK + exc as u64 * EXC + MORPH, back);
                        }
                    }
                }
            }
        }
        // Best accepting node in layer 1.
        let &(key, (cost, _)) = lab[n]
            .iter()
            .filter(|((layer, s), _)| *layer == 1 && mt.accepting(*s))
            .min_by(|a, b| a.1 .0.cmp(&b.1 .0).then(a.0.cmp(&b.0)))?;
        let mut segs = Vec::new();
        let mut cur = (n as u32, key.0, key.1);
        loop {
            let back = lab[cur.0 as usize].iter().find(|(k, _)| *k == (cur.1, cur.2))?.1 .1;
            match back {
                Back::Start => break,
                Back::Morph { from, entry } => {
                    let en = &mt.entries[entry as usize];
                    segs.push(Segment::Morph(Morph {
                        start: from.0 as usize,
                        end: cur.0 as usize,
                        surface: en.surface.clone(),
                        lemma: en.lemma.clone(),
                        tag: en.tag.clone(),
                    }));
                    cur = from;
                }
                Back::Wild { from } => {
                    let (s, e) = (from.0 as usize, cur.0 as usize);
                    segs.push(Segment::Unknown {
                        start: s,
                        end: e,
                        surface: word[s..e].to_string(),
                        guess: {
                            let exc = mt.describe_exceptions(from.2, cur.2);
                            let tag = mt.open_tag(from.2);
                            if exc.is_empty() { tag.to_string() } else { format!("{tag} {exc}") }
                        },
                        pieces: self.trie.tokenize(&word[s..e]),
                    });
                    cur = from;
                }
            }
        }
        segs.reverse();
        // Lexicographic cost means a lone wildcard wins only when no known
        // morpheme fits anywhere; that adds nothing over the plain fallback.
        if segs.len() < 2 {
            return None;
        }
        Some(WordOutput::Hybrid { segments: segs, unknown_chars: (cost / UNK) as u32 })
    }
}
