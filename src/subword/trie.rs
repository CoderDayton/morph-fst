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
        let goto = |u: u32, c: char| self.children[u as usize].get(&c).copied();

        let mut fail = vec![NONE; n];
        let mut pops: Vec<Vec<u32>> = vec![Vec::new(); n];
        let mut queue: VecDeque<u32> = VecDeque::from([ROOT_INIT, ROOT_CONT]);
        while let Some(u) = queue.pop_front() {
            for (&c, &v) in &self.children[u as usize] {
                queue.push_back(v);
                let vi = v as usize;
                if self.piece[vi] != NONE {
                    fail[vi] = ROOT_CONT;
                    pops[vi] = vec![self.piece[vi]];
                    continue;
                }
                if u == ROOT_INIT || u == ROOT_CONT {
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
                    z = fail[z as usize];
                };
                if let Some(t) = target {
                    fail[vi] = t;
                    pops[vi] = acc;
                }
            }
        }
        let mut pops_off = vec![0u32];
        let mut pop_pool = Vec::new();
        for p in &pops {
            pop_pool.extend_from_slice(p);
            pops_off.push(pop_pool.len() as u32);
        }
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
        let mut depth = vec![u32::MAX; n];
        depth[0] = 0;
        depth[1] = 0;
        let mut q = VecDeque::from([0u32, 1u32]);
        let mut seen = 2;
        while let Some(u) = q.pop_front() {
            for &v in &targets[child_off[u as usize] as usize..child_off[u as usize + 1] as usize] {
                depth[v as usize] = depth[u as usize] + 1;
                seen += 1;
                q.push_back(v);
            }
        }
        if seen != n {
            return art_err("unreachable trie nodes");
        }
        for v in 0..n {
            let is_root = v < 2;
            if is_root {
                if fail[v] != NONE || piece[v] != NONE {
                    return art_err("roots must have no failure link and no piece");
                }
                continue;
            }
            if piece[v] != NONE && piece[v] as usize >= pieces.len() {
                return art_err("piece id out of range");
            }
            let f = fail[v];
            if f == NONE || f as usize >= n || depth[f as usize] >= depth[v] {
                return art_err("failure link missing or not depth-decreasing");
            }
        }
        if pop_pool.iter().any(|&p| p as usize >= pieces.len()) {
            return art_err("failure pop out of range");
        }
        if payload_pool.iter().any(|&e| e as usize >= n_entries) {
            return art_err("payload entry out of range");
        }
        Ok(WpTrie { child_off, labels, targets, piece, fail, pops_off, pop_pool, payload_off, payload_pool, pieces })
    }
}
