//! The analysis lattice.
//!
//! Nodes are `(byte position, morphotactic state)`. The scan discovers
//! *arcs* `(start, end, entry)` — "entry's surface occurs at [start, end)" —
//! and feeds them to a [`LatticeBuilder`], which applies morphotactics.
//! The only contract: `add_arc(start, ..)` may be called only once every arc
//! ending at `start` has been added (true for any left-to-right scan).
//!
//! Decoding is deterministic. The best analysis minimises, in order:
//! number of morphemes, then the arc sequence lexicographically with arc key
//! `(longer morpheme first, lower entry id)`.

use std::cmp::{Ordering, Reverse};
use std::collections::BTreeSet;

use crate::morphotactics::Morphotactics;
use crate::output::{Analysis, Morph};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Edge {
    pub from_pos: u32,
    pub from_state: u32,
    pub to_pos: u32,
    pub to_state: u32,
    pub entry: u32,
}

pub struct LatticeBuilder<'a> {
    mt: &'a Morphotactics,
    /// Sorted, deduplicated states per byte position.
    states: Vec<Vec<u32>>,
    edges: Vec<Edge>,
    /// Left context register at each byte offset (empty when unused).
    reg_at: Vec<u16>,
    /// Right context register at each byte offset (empty when unused).
    rreg_at: Vec<u16>,
    multi: Vec<u32>,
}

impl<'a> LatticeBuilder<'a> {
    pub fn new(mt: &'a Morphotactics, word: &str) -> Self {
        let mut states = vec![Vec::new(); word.len() + 1];
        states[0].extend(mt.start_states().iter().copied());
        states[0].sort_unstable();
        states[0].dedup();
        let mut reg_at = Vec::new();
        if mt.has_register() {
            mt.track(word, &mut reg_at);
        }
        let mut rreg_at = Vec::new();
        if mt.has_right_register() {
            mt.track_right(word, &mut rreg_at);
        }
        LatticeBuilder { mt, states, edges: Vec::with_capacity(4 * word.len()), reg_at, rreg_at, multi: Vec::new() }
    }

    #[inline]
    pub fn active(&self, pos: usize) -> bool {
        !self.states[pos].is_empty()
    }

    pub fn add_arc(&mut self, start: usize, end: usize, entry: u32) {
        debug_assert!(start < end);
        let reg = if self.reg_at.is_empty() { 0 } else { self.reg_at[start] as u32 };
        let mt = self.mt;
        let edges = &mut self.edges;
        // start < end: read states[start] in place while filling states[end].
        let (lo, hi) = self.states.split_at_mut(end);
        let (from, to) = (&lo[start], &mut hi[0]);
        let insert = |to: &mut Vec<u32>, t: u32| {
            if let Err(i) = to.binary_search(&t) {
                to.insert(i, t);
            }
        };
        if !self.rreg_at.is_empty() || mt.needs_multi() {
            let rreg = if self.rreg_at.is_empty() { 0 } else { self.rreg_at[end] as u32 };
            let multi = &mut self.multi;
            for &s in from.iter() {
                multi.clear();
                if mt.needs_multi() {
                    mt.step_multi(s, entry, reg, rreg, multi);
                } else if let Some(t) = mt.step(s, entry, reg, rreg) {
                    multi.push(t);
                }
                for &t in multi.iter() {
                    for t2 in mt.closure(t) {
                        insert(to, t2);
                        edges.push(Edge { from_pos: start as u32, from_state: s, to_pos: end as u32, to_state: t2, entry });
                    }
                }
            }
            return;
        }
        for &s in from.iter() {
            if let Some(t) = mt.step(s, entry, reg, 0) {
                for t2 in mt.closure(t) {
                    insert(to, t2);
                    edges.push(Edge { from_pos: start as u32, from_state: s, to_pos: end as u32, to_state: t2, entry });
                }
            }
        }
    }

    pub fn finish(self) -> Lattice {
        let mut edges = self.edges;
        edges.sort_unstable();
        edges.dedup();
        Lattice::new(self.states, edges)
    }
}

pub struct Lattice {
    pub n: u32,
    pub states: Vec<Vec<u32>>,
    pub edges: Vec<Edge>,
    node_off: Vec<usize>,
    to_node: Vec<usize>,
    out_off: Vec<usize>,
}

const NO: usize = usize::MAX;

impl Lattice {
    fn new(states: Vec<Vec<u32>>, edges: Vec<Edge>) -> Self {
        let n = (states.len() - 1) as u32;
        let mut node_off = Vec::with_capacity(states.len() + 1);
        let mut acc = 0;
        for s in &states {
            node_off.push(acc);
            acc += s.len();
        }
        node_off.push(acc);
        let node = |pos: u32, s: u32| -> Option<usize> {
            let i = states.get(pos as usize)?.binary_search(&s).ok()?;
            Some(node_off[pos as usize] + i)
        };
        // Edges are sorted by (from_pos, from_state) == node order. Every
        // endpoint is a node by construction; an edge that is not is dropped.
        let mut kept = Vec::with_capacity(edges.len());
        let mut to_node = Vec::with_capacity(edges.len());
        let mut out_off = vec![0usize; acc + 1];
        for e in edges {
            let (Some(from), Some(to)) = (node(e.from_pos, e.from_state), node(e.to_pos, e.to_state)) else { continue };
            out_off[from + 1] += 1;
            kept.push(e);
            to_node.push(to);
        }
        for i in 0..acc {
            out_off[i + 1] += out_off[i];
        }
        Lattice { n, states, edges: kept, node_off, to_node, out_off }
    }

    fn n_nodes(&self) -> usize {
        self.node_off.last().copied().unwrap_or(0)
    }

    fn node_pos(&self, v: usize) -> u32 {
        (self.node_off.partition_point(|&o| o <= v) - 1) as u32
    }

    fn node_state(&self, v: usize) -> u32 {
        let p = self.node_pos(v) as usize;
        self.states[p][v - self.node_off[p]]
    }

    pub fn accepts(&self, mt: &Morphotactics) -> bool {
        self.states[self.n as usize].iter().any(|&s| mt.accepting(s))
    }

    #[inline]
    fn key(&self, e: usize) -> (Reverse<u32>, u32) {
        (Reverse(self.edges[e].to_pos), self.edges[e].entry)
    }

    /// Backward pass: co-reachability + best continuation per node.
    fn solve(&self, mt: &Morphotactics) -> (Vec<bool>, Vec<u32>, Vec<usize>) {
        let nn = self.n_nodes();
        let mut good = vec![false; nn];
        let mut len = vec![u32::MAX; nn];
        let mut next = vec![NO; nn];
        let last = self.node_off[self.n as usize];
        for v in last..nn {
            if mt.accepting(self.node_state(v)) {
                good[v] = true;
                len[v] = 0;
            }
        }
        for v in (0..last).rev() {
            for e in self.out_off[v]..self.out_off[v + 1] {
                let t = self.to_node[e];
                if !good[t] {
                    continue;
                }
                let cand_len = len[t] + 1;
                let better = if !good[v] {
                    true
                } else if cand_len != len[v] {
                    cand_len < len[v]
                } else {
                    match self.key(e).cmp(&self.key(next[v])) {
                        Ordering::Less => true,
                        Ordering::Greater => false,
                        Ordering::Equal => self.chain_cmp(&next, next[t], next[self.to_node[next[v]]]) == Ordering::Less,
                    }
                };
                if better {
                    good[v] = true;
                    len[v] = cand_len;
                    next[v] = e;
                }
            }
        }
        (good, len, next)
    }

    fn chain_cmp(&self, next: &[usize], mut a: usize, mut b: usize) -> Ordering {
        loop {
            match (a == NO, b == NO) {
                (true, true) => return Ordering::Equal,
                (true, false) => return Ordering::Less,
                (false, true) => return Ordering::Greater,
                _ => {}
            }
            let k = self.key(a).cmp(&self.key(b));
            if k != Ordering::Equal {
                return k;
            }
            a = next[self.to_node[a]];
            b = next[self.to_node[b]];
        }
    }

    fn arcs_to_analysis(&self, mt: &Morphotactics, arcs: &[(u32, u32, u32)]) -> Analysis {
        let mut out = Vec::with_capacity(arcs.len());
        for &(s, e, id) in arcs {
            let en = &mt.entries[id as usize];
            out.push(Morph { start: s as usize, end: e as usize, surface: en.surface.clone(), lemma: en.lemma.clone(), tag: en.tag.clone() });
            out.extend(en.zeros.iter().map(|z| Morph::zero(e as usize, z)));
        }
        out
    }

    fn arc(&self, e: usize) -> (u32, u32, u32) {
        let x = &self.edges[e];
        (x.from_pos, x.to_pos, x.entry)
    }

    /// Best analysis plus up to `cap` analyses in canonical order.
    pub fn decode(&self, mt: &Morphotactics, cap: usize) -> Option<(Analysis, Vec<Analysis>)> {
        if !self.accepts(mt) {
            return None;
        }
        let (good, len, next) = self.solve(mt);
        // Best start node.
        let mut best: Option<usize> = None;
        for v in 0..self.node_off[1] {
            if !good[v] {
                continue;
            }
            best = Some(match best {
                None => v,
                Some(b) => {
                    let ord = len[v].cmp(&len[b]).then_with(|| self.chain_cmp(&next, next[v], next[b]));
                    if ord == Ordering::Less {
                        v
                    } else {
                        b
                    }
                }
            });
        }
        let mut best_arcs = Vec::new();
        let mut cur = next[best?];
        while cur != NO {
            best_arcs.push(self.arc(cur));
            cur = next[self.to_node[cur]];
        }

        // Bounded enumeration (DFS over co-reachable nodes).
        const ENUM_LIMIT: usize = 512;
        let mut budget: usize = 200_000;
        let mut found: BTreeSet<Vec<(u32, u32, u32)>> = BTreeSet::new();
        found.insert(best_arcs.clone());
        let last = self.node_off[self.n as usize];
        'outer: for v in 0..self.node_off[1] {
            if !good[v] {
                continue;
            }
            let mut stack: Vec<(usize, usize)> = vec![(v, self.out_off[v])];
            let mut path: Vec<usize> = Vec::new();
            while let Some(top) = stack.last_mut() {
                if budget == 0 || found.len() >= ENUM_LIMIT {
                    break 'outer;
                }
                budget -= 1;
                let (node, cur) = *top;
                if node >= last {
                    found.insert(path.iter().map(|&e| self.arc(e)).collect());
                    stack.pop();
                    path.pop();
                    continue;
                }
                if cur == self.out_off[node + 1] {
                    stack.pop();
                    path.pop();
                    continue;
                }
                top.1 += 1;
                let t = self.to_node[cur];
                if good[t] {
                    path.push(cur);
                    stack.push((t, self.out_off[t]));
                }
            }
        }
        let mut all: Vec<Vec<(u32, u32, u32)>> = found.into_iter().collect();
        all.sort_by(|a, b| {
            a.len().cmp(&b.len()).then_with(|| {
                let ka = a.iter().map(|&(_, e, id)| (Reverse(e), id));
                let kb = b.iter().map(|&(_, e, id)| (Reverse(e), id));
                ka.cmp(kb)
            })
        });
        all.truncate(cap.max(1));
        let all = all.iter().map(|a| self.arcs_to_analysis(mt, a)).collect();
        Some((self.arcs_to_analysis(mt, &best_arcs), all))
    }

    /// Canonical arc set.
    pub fn arc_set(&self) -> BTreeSet<(u32, u32, u32)> {
        self.edges.iter().map(|e| (e.from_pos, e.to_pos, e.entry)).collect()
    }
}
