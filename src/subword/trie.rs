//! WordPiece trie with failure links (+ optional morpheme payloads).

use std::collections::{BTreeMap, VecDeque};

use crate::codec::{Reader, Writer};
use crate::error::{art_err, Result};
use crate::morphotactics::check_csr;
use crate::output::SubToken;

pub const ROOT_INIT: u32 = 0;
pub const ROOT_CONT: u32 = 1;
pub const NONE: u32 = u32::MAX;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PieceDef {
    pub text: String,
    pub cont: bool,
}

/// Mutable build-time trie.
pub struct TrieBuilder {
    children: Vec<BTreeMap<char, u32>>,
    piece: Vec<u32>,
    payload: Vec<Vec<u32>>,
    pieces: Vec<PieceDef>,
}

impl Default for TrieBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TrieBuilder {
    pub fn new() -> Self {
        TrieBuilder {
            children: vec![BTreeMap::new(), BTreeMap::new()],
            piece: vec![NONE, NONE],
            payload: vec![Vec::new(), Vec::new()],
            pieces: Vec::new(),
        }
    }

    fn insert(&mut self, root: u32, key: &str) -> u32 {
        let mut u = root;
        for c in key.chars() {
            u = match self.children[u as usize].get(&c) {
                Some(&v) => v,
                None => {
                    let v = self.children.len() as u32;
                    self.children.push(BTreeMap::new());
                    self.piece.push(NONE);
                    self.payload.push(Vec::new());
                    self.children[u as usize].insert(c, v);
                    v
                }
            };
        }
        u
    }

    /// Register a piece (`cont` = `##` piece). Returns its id.
    pub fn add_piece(&mut self, text: &str, cont: bool) -> u32 {
        assert!(!text.is_empty(), "empty piece");
        let v = self.insert(if cont { ROOT_CONT } else { ROOT_INIT }, text);
        if self.piece[v as usize] == NONE {
            self.piece[v as usize] = self.pieces.len() as u32;
            self.pieces.push(PieceDef { text: text.to_string(), cont });
        }
        self.piece[v as usize]
    }

    /// Attach a morpheme entry id to the node for `surface` (under ROOT_INIT).
    pub fn add_payload(&mut self, surface: &str, entry: u32) {
        assert!(!surface.is_empty(), "empty surface");
        let v = self.insert(ROOT_INIT, surface);
        self.payload[v as usize].push(entry);
    }

    /// Unified insertion used by the fused compiler: one key, inserted once,
    /// optionally marked as a piece and/or carrying morpheme payloads.
    pub fn add_key(&mut self, text: &str, cont: bool, is_piece: bool, payload: &[u32]) {
        assert!(!text.is_empty(), "empty key");
        let v = self.insert(if cont { ROOT_CONT } else { ROOT_INIT }, text) as usize;
        if is_piece && self.piece[v] == NONE {
            self.piece[v] = self.pieces.len() as u32;
            self.pieces.push(PieceDef { text: text.to_string(), cont });
        }
        self.payload[v].extend_from_slice(payload);
    }

    /// The single finalisation pass: CSR layout + failure links + failure
    /// pops (BFS in depth order over both roots) + payload packing.
    pub fn finish(self) -> WpTrie {
        let n = self.children.len();
        let mut child_off = Vec::with_capacity(n + 1);
        let mut labels = Vec::new();
        let mut targets = Vec::new();
        child_off.push(0u32);
        for ch in &self.children {
            for (&c, &v) in ch {
                labels.push(c as u32);
                targets.push(v);
            }
            child_off.push(labels.len() as u32);
        }
        debug_assert_eq!(n + 1, child_off.len());
        let (fail, pops_off, pop_pool) = links(&child_off, &labels, &targets, &self.piece, None);
        let mut payload_off = vec![0u32];
        let mut payload_pool = Vec::new();
        for p in &self.payload {
            let mut p = p.clone();
            p.sort_unstable();
            p.dedup();
            payload_pool.extend_from_slice(&p);
            payload_off.push(payload_pool.len() as u32);
        }
        WpTrie {
            child_off,
            labels,
            targets,
            piece: self.piece,
            fail,
            pops_off,
            pop_pool,
            payload_off,
            payload_pool,
            pieces: self.pieces,
        }
    }
}

/// Failure links and pops (BFS in depth order over both roots), from the
/// CSR edges (labels sorted per node) and the piece marks. Returns
/// `(fail, pops_off, pop_pool)`. With `limit` (loading), returns empty
/// arrays as soon as a node gets no link or the pops outgrow `limit`: either
/// is already a mismatch, and stopping keeps a crafted trie from costing
/// quadratic time and memory.
fn links(child_off: &[u32], labels: &[u32], targets: &[u32], piece: &[u32], limit: Option<usize>) -> (Vec<u32>, Vec<u32>, Vec<u32>) {
    let n = piece.len();
    let bad = || (Vec::new(), Vec::new(), Vec::new());
    let mut total = 0usize;
    let over = |len: usize| limit.is_some_and(|l| len > l);
    let span = |u: u32| child_off[u as usize] as usize..child_off[u as usize + 1] as usize;
    let goto = |u: u32, c: u32| {
        let r = span(u);
        labels[r.clone()].binary_search(&c).ok().map(|i| targets[r.start + i])
    };
    let mut fail = vec![NONE; n];
    let mut pops: Vec<Vec<u32>> = vec![Vec::new(); n];
    let mut queue: VecDeque<u32> = VecDeque::from([ROOT_INIT, ROOT_CONT]);
    while let Some(u) = queue.pop_front() {
        for i in span(u) {
            let (c, v) = (labels[i], targets[i]);
            queue.push_back(v);
            let vi = v as usize;
            if piece[vi] != NONE {
                fail[vi] = ROOT_CONT;
                pops[vi] = vec![piece[vi]];
                total += 1;
                if over(total) {
                    return bad();
                }
                continue;
            }
            if u == ROOT_INIT || u == ROOT_CONT {
                if limit.is_some() {
                    return bad();
                }
                continue; // unreachable under closure; leaves fail = NONE
            }
            let mut z = fail[u as usize];
            let mut acc = pops[u as usize].clone();
            let target = loop {
                if z == NONE {
                    break None;
                }
                if let Some(t) = goto(z, c) {
                    break Some(t);
                }
                acc.extend_from_slice(&pops[z as usize]);
                if over(total + acc.len()) {
                    return bad();
                }
                z = fail[z as usize];
            };
            match target {
                Some(t) => {
                    total += acc.len();
                    if over(total) {
                        return bad();
                    }
                    fail[vi] = t;
                    pops[vi] = acc;
                }
                None if limit.is_some() => return bad(),
                None => {}
            }
        }
    }
    let mut pops_off = vec![0u32];
    let mut pop_pool = Vec::new();
    for p in &pops {
        pop_pool.extend_from_slice(p);
        pops_off.push(pop_pool.len() as u32);
    }
    (fail, pops_off, pop_pool)
}

/// Each piece sits at exactly one node: the one its text spells. Runs on
/// edges already checked to form a tree with sorted labels.
fn check_pieces(child_off: &[u32], labels: &[u32], targets: &[u32], piece: &[u32], pieces: &[PieceDef]) -> Result<()> {
    let mut placed = vec![false; pieces.len()];
    for (v, &p) in piece.iter().enumerate() {
        if p == NONE {
            continue;
        }
        let (Some(def), Some(seen)) = (pieces.get(p as usize), placed.get_mut(p as usize)) else { return art_err("piece id out of range") };
        if std::mem::replace(seen, true) {
            return art_err("piece placed at two nodes");
        }
        let mut u = if def.cont { ROOT_CONT } else { ROOT_INIT };
        for c in def.text.chars() {
            let r = child_off[u as usize] as usize..child_off[u as usize + 1] as usize;
            match labels[r.clone()].binary_search(&(c as u32)) {
                Ok(i) => u = targets[r.start + i],
                Err(_) => return art_err("piece text does not spell its trie path"),
            }
        }
        if u as usize != v {
            return art_err("piece text does not spell its trie path");
        }
    }
    if placed.contains(&false) {
        return art_err("piece not placed in the trie");
    }
    Ok(())
}

/// Immutable, flat (CSR) trie: what gets serialized.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WpTrie {
    child_off: Vec<u32>,
    labels: Vec<u32>,
    targets: Vec<u32>,
    piece: Vec<u32>,
    fail: Vec<u32>,
    pops_off: Vec<u32>,
    pop_pool: Vec<u32>,
    payload_off: Vec<u32>,
    payload_pool: Vec<u32>,
    pub pieces: Vec<PieceDef>,
}

impl WpTrie {
    pub fn n_nodes(&self) -> usize {
        self.piece.len()
    }

    #[inline]
    pub fn goto(&self, u: u32, c: char) -> Option<u32> {
        let a = self.child_off[u as usize] as usize;
        let b = self.child_off[u as usize + 1] as usize;
        let ls = &self.labels[a..b];
        let c = c as u32;
        // Linear scan for small fan-out, binary search otherwise.
        if ls.len() <= 8 {
            ls.iter().position(|&l| l == c).map(|i| self.targets[a + i])
        } else {
            ls.binary_search(&c).ok().map(|i| self.targets[a + i])
        }
    }

    #[inline]
    pub fn fail(&self, u: u32) -> u32 {
        self.fail[u as usize]
    }

    #[inline]
    pub fn pops(&self, u: u32) -> &[u32] {
        &self.pop_pool[self.pops_off[u as usize] as usize..self.pops_off[u as usize + 1] as usize]
    }

    #[inline]
    pub fn payload(&self, u: u32) -> &[u32] {
        &self.payload_pool[self.payload_off[u as usize] as usize..self.payload_off[u as usize + 1] as usize]
    }

    pub fn payload_total(&self) -> usize {
        self.payload_pool.len()
    }

    pub fn piece_token(&self, id: u32) -> SubToken {
        let p = &self.pieces[id as usize];
        SubToken::Piece { text: p.text.clone(), cont: p.cont }
    }

    /// Tokenize one word with LinMaxMatch + byte fallback.
    pub fn tokenize(&self, word: &str) -> Vec<SubToken> {
        let mut steps = 0;
        self.tokenize_counted(word, &mut steps)
    }

    /// Same, also counting automaton steps (gotos + failure transitions).
    pub fn tokenize_counted(&self, word: &str, steps: &mut u64) -> Vec<SubToken> {
        let mut out = Vec::new();
        let mut lm = super::LinMax::new();
        for c in word.chars() {
            lm.step(self, c, &mut out, steps);
        }
        lm.finish(self, &mut out, steps);
        out
    }

    pub fn write(&self, w: &mut Writer) {
        for v in [
            &self.child_off,
            &self.labels,
            &self.targets,
            &self.piece,
            &self.fail,
            &self.pops_off,
            &self.pop_pool,
            &self.payload_off,
            &self.payload_pool,
        ] {
            w.u32s(v);
        }
        w.u32(self.pieces.len() as u32);
        for p in &self.pieces {
            w.u8(p.cont as u8);
            w.str(&p.text);
        }
    }

    /// Deserialize and fully validate. `n_entries` bounds payload ids.
    /// A validated trie cannot make LinMaxMatch panic or loop: edges form a
    /// tree over two roots, and every failure link strictly decreases depth.
    /// It also cannot change text: each piece spells the path to its node,
    /// and the failure links and pops are the ones the edges imply.
    pub fn read(r: &mut Reader, n_entries: usize) -> Result<Self> {
        let child_off = r.u32s()?;
        let labels = r.u32s()?;
        let targets = r.u32s()?;
        let piece = r.u32s()?;
        let fail = r.u32s()?;
        let pops_off = r.u32s()?;
        let pop_pool = r.u32s()?;
        let payload_off = r.u32s()?;
        let payload_pool = r.u32s()?;
        let np = r.count(5)?;
        let mut pieces = Vec::with_capacity(np);
        for _ in 0..np {
            let cont = match r.u8()? {
                0 => false,
                1 => true,
                _ => return art_err("bad piece flag"),
            };
            let text = r.string()?;
            if text.is_empty() {
                return art_err("empty piece");
            }
            pieces.push(PieceDef { text, cont });
        }
        let n = piece.len();
        if n < 2 || fail.len() != n || labels.len() != targets.len() {
            return art_err("trie array length mismatch");
        }
        check_csr(&child_off, labels.len(), n)?;
        check_csr(&pops_off, pop_pool.len(), n)?;
        check_csr(&payload_off, payload_pool.len(), n)?;
        // Tree structure + depths.
        let mut parent = vec![NONE; n];
        for u in 0..n {
            let (a, b) = (child_off[u] as usize, child_off[u + 1] as usize);
            for i in a..b {
                if char::from_u32(labels[i]).is_none() {
                    return art_err("edge label is not a char");
                }
                if i > a && labels[i - 1] >= labels[i] {
                    return art_err("edge labels not strictly sorted");
                }
                let v = targets[i] as usize;
                if v >= n || v == ROOT_INIT as usize || v == ROOT_CONT as usize || parent[v] != NONE {
                    return art_err("trie edges do not form a tree");
                }
                parent[v] = u as u32;
            }
        }
        let mut q = VecDeque::from([0u32, 1u32]);
        let mut seen = 2;
        while let Some(u) = q.pop_front() {
            for &v in &targets[child_off[u as usize] as usize..child_off[u as usize + 1] as usize] {
                seen += 1;
                q.push_back(v);
            }
        }
        if seen != n {
            return art_err("unreachable trie nodes");
        }
        // Pieces and failure links are checked against the ones the edges
        // imply, which covers roots, ranges and depth-decreasing links.
        if payload_pool.iter().any(|&e| e as usize >= n_entries) {
            return art_err("payload entry out of range");
        }
        check_pieces(&child_off, &labels, &targets, &piece, &pieces)?;
        let (f, po, pp) = links(&child_off, &labels, &targets, &piece, Some(pop_pool.len()));
        if f != fail || po != pops_off || pp != pop_pool {
            return art_err("failure links do not match the trie");
        }
        Ok(WpTrie { child_off, labels, targets, piece, fail, pops_off, pop_pool, payload_off, payload_pool, pieces })
    }
}
