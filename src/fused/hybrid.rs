//! Hybrid mode: when the strict parse fails, read the word as known
//! morphemes around **one** unknown stem of an open class.
//!
//! An unknown stem is a free *head* followed, when possible, by a stem
//! *shape*: an ending that known stems of the class have, with their
//! spelling, continuation and lemma (see [`crate::shapes`]). So `blorties`
//! reads as `blor` + `tie`→`ty` (like `city`) + `s`, lemma `blorty`.
//!
//! Readings are ranked by the lexicographic cost
//! `(head chars, exceptions, missing shape support, morphemes)` and the best
//! [`ANALYSES_CAP`] distinct ones are kept. The search is best-first over
//! `(position, layer, state)` (layer 0 before the stem, 1 after it) and
//! stops once enough readings are found, so long heads that cannot rank are
//! never built. A full parse has no head, so hybrid ≡ strict whenever strict
//! succeeds.

use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap, HashMap};
use std::hash::{BuildHasherDefault, Hasher};

use super::Lexer;
use crate::output::{Morph, Segment, WordOutput};
use crate::spec::{ClassId, END};
use crate::subword::ROOT_INIT;
use crate::ANALYSES_CAP;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Cost {
    head: u32,
    exc: u32,
    unsupported: u32,
    morphs: u32,
}

impl Cost {
    fn plus(self, o: Cost) -> Cost {
        Cost {
            head: self.head.saturating_add(o.head),
            exc: self.exc.saturating_add(o.exc),
            unsupported: self.unsupported.saturating_add(o.unsupported),
            morphs: self.morphs.saturating_add(o.morphs),
        }
    }
}

#[derive(Clone, Copy)]
enum Step {
    Start,
    Morph { entry: u32 },
    Stem { head_end: u32, shape: Option<u32> },
    /// A queued head ending at `pos`, expanded when popped (layer [`HEAD`]).
    Head,
}

#[derive(Clone, Copy)]
struct Deriv {
    cost: Cost,
    prev: u32,
    pos: u32,
    layer: u8,
    state: u32,
    step: Step,
}

const NONE: u32 = u32::MAX;
const HEAD: u8 = 2;
/// Most derivations expanded per word: a bound for adversarial input.
const POP_BUDGET: usize = 50_000;

/// What a derivation outputs, as integers per step: `[end, id, extra, before
/// state, after state]`. A bare head prints its exceptions from the states
/// around it, so those are part of its signature; other steps print the
/// same whatever the states. Equal signatures print the same reading.
type Signature = Vec<[u32; 5]>;

fn signature(chart: &Chart, mut d: u32) -> Signature {
    let mut sig = Vec::new();
    while let Some(x) = chart.arena.get(d as usize) {
        match x.step {
            Step::Start => break,
            Step::Morph { entry } => sig.push([x.pos, entry, NONE, 0, 0]),
            Step::Stem { head_end, shape: Some(i) } => sig.push([x.pos, head_end, i, 0, 0]),
            Step::Stem { head_end, shape: None } => {
                let before = chart.arena.get(x.prev as usize).map_or(NONE, |p| p.state);
                sig.push([x.pos, head_end, NONE, before, x.state]);
            }
            Step::Head => {}
        }
        d = x.prev;
    }
    sig
}

/// One distinct reading and the derivations behind it.
struct Group {
    segs: Vec<Segment>,
    head: u32,
    exc: u32,
    support: u64,
    morphs: u32,
}

/// Stop once `cap` readings are found and nothing still queued can tie with
/// them on head length (the cost key aggregated support cannot override).
fn enough(best_heads: &[u32], cap: usize, next_head: u32) -> bool {
    best_heads.len() >= cap && best_heads.get(cap - 1).is_some_and(|&h| next_head > h)
}

/// Record a new group's head length, keeping the `cap` smallest, sorted.
fn note_head(best_heads: &mut Vec<u32>, cap: usize, head: u32) {
    let at = best_heads.partition_point(|&h| h <= head);
    best_heads.insert(at, head);
    best_heads.truncate(cap);
}

/// Best-first agenda. Costs never decrease along a path, so the first `k`
/// pops of a `(position, layer, state)` are its `k` best derivations; later
/// ones are skipped.
struct Chart {
    arena: Vec<Deriv>,
    heap: BinaryHeap<Reverse<(Cost, u32)>>,
    /// Keyed by node plus, for queued heads, where the head starts (heads
    /// from different starts expand differently).
    pops: HashMap<(u32, u8, u32, u32), usize, BuildHasherDefault<FxHasher>>,
    k: usize,
}

/// Multiply-rotate hasher for small integer keys (the chart's node ids are
/// not attacker-chosen beyond one word, and SipHash dominated the profile).
#[derive(Default)]
struct FxHasher(u64);

impl Hasher for FxHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }
    fn write_u8(&mut self, i: u8) {
        self.write_u64(i as u64);
    }
    fn write_u32(&mut self, i: u32) {
        self.write_u64(i as u64);
    }
    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

impl Chart {
    fn push(&mut self, d: Deriv) {
        self.heap.push(Reverse((d.cost, self.arena.len() as u32)));
        self.arena.push(d);
    }

    /// Next derivation to expand, if its node has pops left.
    fn pop(&mut self) -> Option<Option<u32>> {
        let Reverse((_, d)) = self.heap.pop()?;
        let Some(x) = self.arena.get(d as usize) else { return Some(None) };
        let start = if x.layer == HEAD { self.arena.get(x.prev as usize).map_or(0, |p| p.pos) } else { 0 };
        let n = self.pops.entry((x.pos, x.layer, x.state, start)).or_insert(0);
        if *n >= self.k {
            return Some(None);
        }
        *n += 1;
        Some(Some(d))
    }
}

#[derive(Default)]
struct Scratch {
    multi: Vec<u32>,
    out: Vec<u32>,
    arcs: Vec<(u32, u32)>,
    /// Per char index of a head end: the shape ranges that continue the word
    /// from there, as (end, range). Filled on first use, per word.
    prefix: Vec<Option<Vec<ShapeSpan>>>,
    /// Reused buffer for unknown-stem states.
    wild: Vec<(u32, u32)>,
}

/// A spelling ending at byte `.0` and the shapes that start with it.
type ShapeSpan = (usize, std::ops::Range<usize>);

/// Per-word context shared by the arc generators.
struct Word<'w> {
    text: &'w str,
    bounds: Vec<usize>,
    reg: Vec<u16>,
    rreg: Vec<u16>,
    /// Trie nodes with payloads reached from byte offset `p`, as (end, node):
    /// `nodes[node_start[p]..node_start[p + 1]]`.
    nodes: Vec<(u32, u32)>,
    node_start: Vec<u32>,
    /// Per byte offset `p`: bitset of the lexicons with a morpheme starting
    /// there, `lex_words` u64s from `p * lex_words`.
    lex_bits: Vec<u64>,
    lex_words: usize,
}

impl Word<'_> {
    fn reg(&self, p: usize) -> u32 {
        self.reg.get(p).copied().unwrap_or(0) as u32
    }
    fn rreg(&self, p: usize) -> u32 {
        self.rreg.get(p).copied().unwrap_or(0) as u32
    }
    /// Index of byte offset `b` in `bounds`.
    fn char_idx(&self, b: usize) -> usize {
        self.bounds.binary_search(&b).unwrap_or_else(|i| i)
    }
    fn span(&self, a: usize, b: usize) -> &str {
        self.text.get(a..b).unwrap_or("")
    }
    fn nodes_at(&self, p: usize) -> &[(u32, u32)] {
        match (self.node_start.get(p), self.node_start.get(p + 1)) {
            (Some(&a), Some(&b)) => self.nodes.get(a as usize..b as usize).unwrap_or(&[]),
            _ => &[],
        }
    }
    fn has_lexicon(&self, p: usize, c: usize) -> bool {
        self.lex_bits.get(p * self.lex_words + c / 64).is_some_and(|b| b >> (c % 64) & 1 == 1)
    }
}

impl Lexer {
    /// [`Mode::Hybrid`](super::Mode::Hybrid) lexing of a normalized word.
    pub(crate) fn lex_hybrid(&self, word: &str) -> WordOutput {
        let strict = self.lex_strict(word);
        if !matches!(strict, WordOutput::Fallback(_)) {
            return strict;
        }
        if word.is_empty() || word.chars().count() > self.mt.max_morph_chars as usize {
            return strict;
        }
        self.hybrid_parse(word, ANALYSES_CAP).unwrap_or(strict)
    }

    fn word_context<'w>(&self, text: &'w str) -> Word<'w> {
        let n = text.len();
        let bounds: Vec<usize> = text.char_indices().map(|(i, _)| i).chain(std::iter::once(n)).collect();
        let mut reg = Vec::new();
        self.mt.track(text, &mut reg);
        let mut rreg = Vec::new();
        self.mt.track_right(text, &mut rreg);
        let lex_words = self.mt.classes.len().div_ceil(64);
        let mut nodes = Vec::new();
        let mut node_start = Vec::with_capacity(n + 2);
        let mut lex_bits = vec![0u64; (n + 1) * lex_words];
        for b in 0..=n {
            node_start.push(nodes.len() as u32);
            let Some(rest) = text.get(b..).filter(|_| b < n) else { continue };
            let mut u = ROOT_INIT;
            for (off, c) in rest.char_indices() {
                match self.trie.goto(u, c) {
                    Some(v) => u = v,
                    None => break,
                }
                let pl = self.trie.payload(u);
                if pl.is_empty() {
                    continue;
                }
                nodes.push(((b + off + c.len_utf8()) as u32, u));
                for &e in pl {
                    let c = self.mt.entries.get(e as usize).map_or(0, |x| x.lexicon as usize);
                    if let Some(w) = lex_bits.get_mut(b * lex_words + c / 64) {
                        *w |= 1 << (c % 64);
                    }
                }
            }
        }
        node_start.push(nodes.len() as u32);
        Word { text, bounds, reg, rreg, nodes, node_start, lex_bits, lex_words }
    }

    /// Call `f(end, entry)` for each known morpheme of lexicon `class`
    /// starting at `p`; stops early when `f` returns false.
    fn each_arc(&self, w: &Word, p: usize, class: ClassId, mut f: impl FnMut(u32, u32) -> bool) {
        for &(end, node) in w.nodes_at(p) {
            let pl = self.trie.payload(node);
            let ids = match self.mt.lexicon_range(pl, class) {
                Some(r) => pl.get(r).unwrap_or(&[]),
                None => pl,
            };
            for &e in ids {
                if self.mt.entries.get(e as usize).is_some_and(|x| x.lexicon == class) && !f(end, e) {
                    return;
                }
            }
        }
    }

    /// Targets of entry `e` from state `s` over `[p, end)`, after closure,
    /// keeping only states that can still lead to a reading.
    fn step_closed(&self, w: &Word, layer: u8, s: u32, e: u32, (p, end): (usize, usize), sc: &mut Scratch) {
        let mt = &self.mt;
        sc.multi.clear();
        if mt.needs_multi() {
            mt.step_multi(s, e, w.reg(p), w.rreg(end), &mut sc.multi);
        } else if let Some(t) = mt.step(s, e, w.reg(p), w.rreg(end)) {
            sc.multi.push(t);
        }
        sc.out.clear();
        for &t in &sc.multi {
            sc.out.extend(mt.closure(t).filter(|&t2| self.alive(w, layer, end, t2)));
        }
    }

    /// Can state `s` in `layer` at byte offset `p` still lead to a reading?
    /// Pruning dead states keeps a grammar with hundreds of paradigm classes
    /// from filling every position with states that go nowhere.
    fn alive(&self, w: &Word, layer: u8, p: usize, s: u32) -> bool {
        let mt = &self.mt;
        if p == w.text.len() {
            return layer == 1 && mt.accepting(s);
        }
        let class = mt.class_of(s);
        if class == END {
            return false;
        }
        if layer == 0 && mt.wildcard_allowed(s) {
            return true;
        }
        w.has_lexicon(p, class as usize)
    }

    fn morph_arcs(&self, w: &Word, chart: &mut Chart, layer: u8, d: u32, sc: &mut Scratch) {
        let Some(&from) = chart.arena.get(d as usize) else { return };
        let p = from.pos as usize;
        let mut arcs = std::mem::take(&mut sc.arcs);
        arcs.clear();
        self.each_arc(w, p, self.mt.class_of(from.state), |end, e| {
            arcs.push((end, e));
            true
        });
        for &(end, e) in &arcs {
            self.step_closed(w, layer, from.state, e, (p, end as usize), sc);
            for &t in &sc.out {
                let cost = from.cost.plus(Cost { morphs: 1, ..Cost::default() });
                chart.push(Deriv { cost, prev: d, pos: end, layer, state: t, step: Step::Morph { entry: e } });
            }
        }
        sc.arcs = arcs;
    }

    /// An unknown stem may start after `d`: queue its one-character head.
    /// Heads are grown lazily, one character per pop, so heads too long to
    /// rank are never expanded.
    fn start_stem(&self, w: &Word, chart: &mut Chart, d: u32) {
        let Some(&from) = chart.arena.get(d as usize) else { return };
        if !self.mt.wildcard_allowed(from.state) {
            return;
        }
        let Some(&h) = w.bounds.get(w.char_idx(from.pos as usize) + 1) else { return };
        let cost = from.cost.plus(Cost { head: 1, ..Cost::default() });
        chart.push(Deriv { cost, prev: d, pos: h as u32, layer: HEAD, state: from.state, step: Step::Head });
    }

    /// Expand the queued head `q`: the head alone, or followed by known stem
    /// endings; then queue the head one character longer.
    fn expand_head(&self, w: &Word, chart: &mut Chart, q: u32, sc: &mut Scratch) {
        let mt = &self.mt;
        let Some(&x) = chart.arena.get(q as usize) else { return };
        let Some(&from) = chart.arena.get(x.prev as usize) else { return };
        let (d, p, h, s) = (x.prev, from.pos as usize, x.pos as usize, from.state);
        let head = x.cost.head - from.cost.head;
        let mut wild = std::mem::take(&mut sc.wild);
        wild.clear();
        mt.wildcard_into(s, w.span(p, h), &mut wild);
        for &(t, exc) in wild.iter().filter(|&&(t, _)| self.alive(w, 1, h, t)) {
            let cost = from.cost.plus(Cost { head, exc, unsupported: u32::MAX, morphs: 1 });
            chart.push(Deriv { cost, prev: d, pos: h as u32, layer: 1, state: t, step: Step::Stem { head_end: h as u32, shape: None } });
        }
        sc.wild = wild;
        let k = w.char_idx(h);
        let ranges = match sc.prefix.get_mut(k).and_then(Option::take) {
            Some(r) => r,
            None => {
                let mut out = Vec::new();
                let mut range = 0..mt.shapes.len();
                for &r in w.bounds.iter().skip(k + 1) {
                    range = crate::shapes::with_prefix(&mt.shapes, range, w.span(h, r));
                    if range.is_empty() {
                        break;
                    }
                    out.push((r, range.clone()));
                }
                out
            }
        };
        for (r, range) in &ranges {
            self.shape_arcs(w, chart, q, *r, range.clone(), sc);
        }
        if let Some(slot) = sc.prefix.get_mut(k) {
            *slot = Some(ranges);
        }
        if let Some(&h2) = w.bounds.get(k + 1) {
            let cost = x.cost.plus(Cost { head: 1, ..Cost::default() });
            chart.push(Deriv { cost, prev: d, pos: h2 as u32, layer: HEAD, state: s, step: Step::Head });
        }
    }

    /// Queued head `q` spanning `[from.pos, h)` followed by a known stem
    /// ending spelled `[h, r)`; `range` holds the shapes starting with that
    /// spelling, exact ones first.
    fn shape_arcs(&self, w: &Word, chart: &mut Chart, q: u32, r: usize, range: std::ops::Range<usize>, sc: &mut Scratch) {
        let mt = &self.mt;
        let Some(&x) = chart.arena.get(q as usize) else { return };
        let Some(&from) = chart.arena.get(x.prev as usize) else { return };
        let (d, h, head) = (x.prev, x.pos as usize, x.cost.head - from.cost.head);
        let class = mt.class_of(from.state);
        let spelled = w.span(h, r);
        for i in range {
            let Some(sh) = mt.shapes.get(i) else { break };
            if sh.surface != spelled {
                break;
            }
            if sh.class != class {
                continue;
            }
            self.step_closed(w, 1, from.state, sh.entry, (h, r), sc);
            for &t in &sc.out {
                let cost = from.cost.plus(Cost { head, exc: 0, unsupported: u32::MAX - sh.support, morphs: 1 });
                let step = Step::Stem { head_end: h as u32, shape: Some(i as u32) };
                chart.push(Deriv { cost, prev: d, pos: r as u32, layer: 1, state: t, step });
            }
        }
    }

    fn hybrid_parse(&self, text: &str, cap: usize) -> Option<WordOutput> {
        let mt = &self.mt;
        let w = self.word_context(text);
        let n = text.len() as u32;
        let cap = cap.max(1);
        let mut chart = Chart { arena: Vec::new(), heap: BinaryHeap::new(), pops: HashMap::default(), k: cap };
        for &s in mt.start_states() {
            chart.push(Deriv { cost: Cost::default(), prev: NONE, pos: 0, layer: 0, state: s, step: Step::Start });
        }
        let mut sc = Scratch { prefix: vec![None; w.bounds.len()], ..Scratch::default() };
        let mut groups: Vec<Group> = Vec::new();
        let mut best_heads: Vec<u32> = Vec::new();
        let mut seen: BTreeSet<Signature> = BTreeSet::new();
        let mut budget = POP_BUDGET;
        while let Some(next) = chart.pop() {
            let Some(d) = next else { continue };
            let Some(&x) = chart.arena.get(d as usize) else { continue };
            budget = budget.saturating_sub(1);
            if budget == 0 || enough(&best_heads, cap, x.cost.head) {
                break;
            }
            if x.layer == HEAD {
                self.expand_head(&w, &mut chart, d, &mut sc);
                continue;
            }
            if x.pos < n {
                self.morph_arcs(&w, &mut chart, x.layer, d, &mut sc);
                if x.layer == 0 {
                    self.start_stem(&w, &mut chart, d);
                }
                continue;
            }
            if x.layer != 1 || !mt.accepting(x.state) || !seen.insert(signature(&chart, d)) {
                continue;
            }
            let Some(segs) = self.segments(&w, &chart, d) else { continue };
            // A bare head alone says nothing the plain fallback does not.
            if segs.len() == 1 && matches!(x.step, Step::Stem { shape: None, .. }) {
                continue;
            }
            let support = self.stem_support(&chart, d);
            match groups.iter_mut().find(|g| g.segs == segs) {
                Some(g) => {
                    if (g.head, g.exc) == (x.cost.head, x.cost.exc) {
                        g.support += support;
                    }
                }
                None => {
                    note_head(&mut best_heads, cap, x.cost.head);
                    groups.push(Group { segs, head: x.cost.head, exc: x.cost.exc, support, morphs: x.cost.morphs });
                }
            }
        }
        // Same head length: the reading more known stems support comes first.
        groups.sort_by_key(|g| (g.head, g.exc, Reverse(g.support), g.morphs));
        groups.truncate(cap);
        let unknown_chars = groups.first()?.head;
        let all: Vec<Vec<Segment>> = groups.into_iter().map(|g| g.segs).collect();
        let segments = all.first()?.clone();
        Some(WordOutput::Hybrid { segments, all, unknown_chars })
    }

    /// Support of the stem shape a derivation uses (0 for a bare head).
    fn stem_support(&self, chart: &Chart, mut d: u32) -> u64 {
        while let Some(x) = chart.arena.get(d as usize) {
            if let Step::Stem { shape, .. } = x.step {
                return shape.and_then(|i| self.mt.shapes.get(i as usize)).map_or(0, |s| s.support as u64);
            }
            d = x.prev;
        }
        0
    }

    /// Walk a derivation back to its segments.
    fn segments(&self, w: &Word, chart: &Chart, mut d: u32) -> Option<Vec<Segment>> {
        let mt = &self.mt;
        let mut segs = Vec::new();
        loop {
            let x = chart.arena.get(d as usize)?;
            if matches!(x.step, Step::Start) {
                break;
            }
            let from = chart.arena.get(x.prev as usize)?;
            let (a, b) = (from.pos as usize, x.pos as usize);
            let (entry, seg) = match x.step {
                Step::Morph { entry } => {
                    let en = mt.entries.get(entry as usize)?;
                    (Some(entry), Segment::Morph(Morph { start: a, end: b, surface: en.surface.clone(), lemma: en.lemma.clone(), tag: en.tag.clone() }))
                }
                Step::Stem { head_end, shape } => self.stem_segment(w, from.state, x.state, (a, head_end as usize, b), shape)?,
                Step::Start | Step::Head => break,
            };
            if let Some(en) = entry.and_then(|e| mt.entries.get(e as usize)) {
                segs.extend(en.zeros.iter().rev().map(|z| Segment::Morph(Morph::zero(b, z))));
            }
            segs.push(seg);
            d = x.prev;
        }
        segs.reverse();
        Some(segs)
    }

    /// The unknown stem `[a, b)` with head `[a, h)`, and the entry whose zero
    /// morphemes follow it (its shape's entry).
    fn stem_segment(&self, w: &Word, before: u32, after: u32, (a, h, b): (usize, usize, usize), shape: Option<u32>) -> Option<(Option<u32>, Segment)> {
        let mt = &self.mt;
        let head = w.span(a, h);
        let (entry, lemma, guess) = match shape.and_then(|i| mt.shapes.get(i as usize)) {
            Some(sh) => (Some(sh.entry), format!("{head}{}", sh.lemma), mt.entries.get(sh.entry as usize)?.tag.clone()),
            None => {
                let exc = mt.describe_exceptions(before, after);
                let tag = mt.open_tag(before);
                (None, head.to_string(), if exc.is_empty() { tag.to_string() } else { format!("{tag} {exc}") })
            }
        };
        let surface = w.span(a, b).to_string();
        let pieces = self.trie.tokenize(&surface);
        Some((entry, Segment::Unknown { start: a, end: b, surface, lemma, guess, pieces }))
    }
}
