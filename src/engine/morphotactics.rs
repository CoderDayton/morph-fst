//! Runtime morphotactics: the finite-state glue between morphemes, plus
//! the context registers.
//!
//! A morphotactic **state** packs `(class, filter, pin, rpin)` into a `u32`
//! with shifts (class in the high bits, so states sort by class):
//!
//! * epsilon: `(c, f, p, rp) -> (c', f, p, rp)` (pre-closed into a table);
//! * morpheme `e` on `[i, j)`: allowed iff `class == e.lexicon`, the filter
//!   permits `e.lex_id`, and `e`'s guard contains the effective pair
//!   `(left register at i, right register at j)`.
//!
//! The registers themselves are **not** part of the state: the left one is
//! a function of the text before a position, the right one of the text
//! after it, so each is computed once per word ([`Morphotactics::track`],
//! [`Morphotactics::track_right`]). Only lexical exceptions live in the
//! state: `@set` pins (`pin`) and `@rset` pins (`rpin`). Right pins are
//! *guessed* left to right (per feature: ⊤ = nobody to the left cares,
//! "unpinned", or pinned to a value) and verified by later morphemes and
//! at the end of the word.

use crate::codec::{Reader, Writer};
use crate::error::{art_err, Result};
use crate::normalize::NormOptions;
use crate::shapes::{self, Shape};
use crate::spec::{CaseMode, ClassId, Filter, FilterKind, Grammar, Tracker, END};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassRt {
    pub name: String,
    pub open_tag: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryRt {
    pub lexicon: ClassId,
    pub cont: ClassId,
    pub filter: u16,
    pub lex_id: u32,
    /// 0 = any register pair.
    pub guard: u32,
    pub surface: String,
    pub lemma: String,
    pub tag: String,
    /// Tags of the zero morphemes that follow this one.
    pub zeros: Vec<String>,
}

/// A compiled register automaton (one direction).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegTable {
    pub n_regs: u32,
    n_cols: u32,
    columns: Vec<(u32, u16)>,
    ascii_col: Vec<u16>,
    trans: Vec<u16>,
    /// (feature name, value names) for display.
    pub features: Vec<(String, Vec<String>)>,
}

impl RegTable {
    fn from_tracker(t: &Tracker) -> Self {
        let mut ascii_col = vec![0u16; 128];
        for &(c, col) in &t.columns {
            if c < 128 {
                ascii_col[c as usize] = col;
            }
        }
        RegTable {
            n_regs: t.n_regs,
            n_cols: t.n_cols as u32,
            columns: t.columns.clone(),
            ascii_col,
            trans: t.trans.clone(),
            features: t.feat_names.iter().cloned().zip(t.feat_values.iter().cloned()).collect(),
        }
    }

    #[inline]
    fn col(&self, c: char) -> u32 {
        let u = c as u32;
        if u < 128 {
            return self.ascii_col[u as usize] as u32;
        }
        match self.columns.binary_search_by_key(&u, |&(k, _)| k) {
            Ok(i) => self.columns[i].1 as u32,
            Err(_) => 0,
        }
    }

    #[inline]
    fn step(&self, r: u32, c: char) -> u32 {
        self.trans[r as usize * self.n_cols as usize + self.col(c) as usize] as u32
    }

    fn write(&self, w: &mut Writer) {
        w.u32(self.n_regs);
        w.u32(self.n_cols);
        w.u32(self.columns.len() as u32);
        for &(c, col) in &self.columns {
            w.u32(c);
            w.u32(col as u32);
        }
        w.u32s(&self.trans.iter().map(|&x| x as u32).collect::<Vec<_>>());
        w.u32(self.features.len() as u32);
        for (n, vs) in &self.features {
            w.str(n);
            w.u32(vs.len() as u32);
            for v in vs {
                w.str(v);
            }
        }
    }

    fn read(r: &mut Reader) -> Result<Self> {
        let n_regs = r.u32()?;
        let n_cols = r.u32()?;
        let ncl = r.count(8)?;
        let mut columns = Vec::with_capacity(ncl);
        for _ in 0..ncl {
            let c = r.u32()?;
            let col = r.u32()?;
            if col >= n_cols || col > u16::MAX as u32 || char::from_u32(c).is_none() {
                return art_err("bad register column");
            }
            columns.push((c, col as u16));
        }
        if columns.windows(2).any(|w| w[0].0 >= w[1].0) {
            return art_err("register columns not sorted");
        }
        let trans: Vec<u16> = r.u32s()?.into_iter().map(|x| x.min(u16::MAX as u32) as u16).collect();
        let nfeat = r.count(8)?;
        let mut features = Vec::with_capacity(nfeat);
        for _ in 0..nfeat {
            let n = r.string()?;
            let nv = r.count(4)?;
            let mut vs = Vec::with_capacity(nv);
            for _ in 0..nv {
                vs.push(r.string()?);
            }
            features.push((n, vs));
        }
        if n_regs == 0 || n_regs > crate::spec::expand::MAX_REGS || n_cols == 0 {
            return art_err("bad register size");
        }
        if trans.len() as u64 != n_regs as u64 * n_cols as u64 || trans.iter().any(|&x| x as u32 >= n_regs) {
            return art_err("bad register transition table");
        }
        let mut ascii_col = vec![0u16; 128];
        for &(c, col) in &columns {
            if c < 128 {
                ascii_col[c as usize] = col;
            }
        }
        Ok(RegTable { n_regs, n_cols, columns, ascii_col, trans, features })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Morphotactics {
    pub classes: Vec<ClassRt>,
    pub entries: Vec<EntryRt>,
    pub filters: Vec<Filter>,
    cc_off: Vec<u32>,
    cc_pool: Vec<u32>,
    start: Vec<u32>,
    wild_off: Vec<u32>,
    wild_pool: Vec<u32>,
    pub max_morph_chars: u32,
    fb: u32,
    pb: u32,
    rb: u32,
    /// Underlying mode: registers live in the state (bits `lb`, `qb`).
    under: bool,
    lb: u32,
    qb: u32,
    // derived (not serialized): shifts/masks and fast-path flags
    sh: u32,
    fsh: u32,
    psh: u32,
    rsh: u32,
    lsh: u32,
    fmask: u32,
    pmask: u32,
    rmask: u32,
    lmask: u32,
    qmask: u32,
    ctx: bool,
    multi: bool,
    rn: u32,
    rpins_on: bool,
    /// Left-context register (FEATUREs).
    pub left: RegTable,
    /// Right-context register (RFEATUREs), run from the word's end.
    pub right: RegTable,
    guard_words: u32,
    guard_pool: Vec<u64>,
    // left pins
    n_pins: u32,
    pin_apply: Vec<u16>,
    pin_next: Vec<u16>,
    // right pins (guess-and-verify)
    n_rpins: u32,
    /// `rpin_apply[rp * R_r + q]`: effective right register.
    rpin_apply: Vec<u16>,
    /// Codes whose features are all ⊤ or unpinned (word end / unknown stem).
    rpin_free: Vec<bool>,
    /// Per (entry, rpin at its start): CSR list of consistent rpin at its end.
    rsucc_off: Vec<u32>,
    rsucc_pool: Vec<u16>,
    // underlying mode
    /// `next_l[e * R_l + r]`: left register after entry `e`'s tier.
    next_l: Vec<u16>,
    /// Per (entry, right register at its start): right registers at its end
    /// consistent with its tier (CSR).
    rinv_off: Vec<u32>,
    rinv_pool: Vec<u16>,
    reads_right: Vec<u8>,
    // INDUCE (exceptions of unknown stems)
    /// Left pin codes an unknown stem may set, with their exception count.
    wild_pins: Vec<(u32, u32)>,
    /// Per right-pin code: exception count if an unknown stem may stand
    /// behind it, or u32::MAX.
    rpin_wild: Vec<u32>,
    /// Pinnable features (name, value names) for describing exceptions.
    pin_feats: Vec<(String, Vec<String>)>,
    rpin_feats: Vec<(String, Vec<String>)>,
    pub norm: NormOptions,
    /// Tags of derivational morphemes (see `Options::derivation`).
    pub derivation: Vec<String>,
    /// `PREFER` rules, in order (see `Options::context`).
    pub context: Vec<crate::spec::ContextRule>,
    /// Stem shapes of the open lexicons, sorted by surface (hybrid mode).
    pub shapes: Vec<Shape>,
    /// Each class's distinct shape prefixes, sorted (derived).
    pub shape_pre: Vec<(ClassId, String)>,
    /// Entry ids grouped by lexicon in ascending order (derived).
    lex_sorted: bool,
    /// Lexicon of each entry, compact for binary searches (derived).
    lex_of: Vec<ClassId>,
}

fn bits(n: usize) -> u32 {
    if n <= 1 {
        0
    } else {
        usize::BITS - (n - 1).leading_zeros()
    }
}

impl Morphotactics {
    pub fn from_grammar(g: &Grammar) -> Self {
        let nc = g.classes.len();
        let mut cc_off = vec![0u32];
        let mut cc_pool = Vec::new();
        for c in 0..nc {
            cc_pool.extend(g.class_closure(c as ClassId).into_iter().map(u32::from));
            cc_off.push(cc_pool.len() as u32);
        }
        let t = &g.tracker;
        let rt = &g.rtracker;
        // ---- left pins (digit 0 = unpinned, v+1 = pinned to v) ----
        let pin_sizes: Vec<u32> = g.pinnable.iter().map(|&f| t.feat_values[f as usize].len() as u32 + 1).collect();
        let n_pins: u32 = pin_sizes.iter().product();
        let mut pin_apply = Vec::new();
        let mut pin_next = Vec::new();
        if n_pins > 1 {
            for p in 0..n_pins {
                let d = decode(p, &pin_sizes);
                for r in 0..t.n_regs {
                    let mut x = r;
                    for (i, &f) in g.pinnable.iter().enumerate() {
                        if d[i] > 0 {
                            x = with_value(t, x, f as usize, d[i] - 1);
                        }
                    }
                    pin_apply.push(x as u16);
                }
            }
            for e in &g.entries {
                for p in 0..n_pins {
                    let mut d = decode(p, &pin_sizes);
                    for (i, di) in d.iter_mut().enumerate() {
                        if e.pin_touch >> i & 1 == 1 {
                            *di = 0;
                        }
                    }
                    for &(i, v) in &e.pin_set {
                        d[i as usize] = v as u32 + 1;
                    }
                    pin_next.push(encode(&d, &pin_sizes) as u16);
                }
            }
        }
        // ---- right pins (digit 0 = ⊤, 1 = unpinned, v+2 = pinned to v) ----
        let rsizes: Vec<u32> = g.rpinnable.iter().map(|&f| rt.feat_values[f as usize].len() as u32 + 2).collect();
        let n_rpins: u32 = rsizes.iter().product();
        let mut rpin_apply = Vec::new();
        let mut rpin_free = vec![true];
        let mut rsucc_off = Vec::new();
        let mut rsucc_pool = Vec::new();
        if n_rpins > 1 {
            rpin_free = (0..n_rpins).map(|p| decode(p, &rsizes).iter().all(|&d| d <= 1)).collect();
            for p in 0..n_rpins {
                let d = decode(p, &rsizes);
                for q in 0..rt.n_regs {
                    let mut x = q;
                    for (i, &f) in g.rpinnable.iter().enumerate() {
                        if d[i] >= 2 {
                            x = with_value(rt, x, f as usize, d[i] - 2);
                        }
                    }
                    rpin_apply.push(x as u16);
                }
            }
            rsucc_off.push(0);
            for e in &g.entries {
                for a in 0..n_rpins {
                    let ad = decode(a, &rsizes);
                    // per feature: allowed digits for the rpin at e's end
                    let mut options: Vec<Vec<u32>> = Vec::new();
                    let mut ok = true;
                    for (i, &ai) in ad.iter().enumerate() {
                        let concrete: Vec<u32> = (1..rsizes[i]).collect();
                        let free = if e.reads_right { concrete.clone() } else { vec![0] };
                        let set = e.rpin_set.iter().find(|&&(f, _)| f as usize == i).map(|&(_, v)| v as u32 + 2);
                        let o = if let Some(w) = set {
                            // this morpheme pins the feature for its left side
                            if ai != 0 && ai != w {
                                ok = false;
                            }
                            free
                        } else if e.rpin_touch >> i & 1 == 1 {
                            // its own text overrides any pin from the right
                            if ai != 0 && ai != 1 {
                                ok = false;
                            }
                            free
                        } else if ai == 0 {
                            free
                        } else {
                            vec![ai] // passes through unchanged
                        };
                        options.push(o);
                    }
                    if ok {
                        let mut acc = vec![Vec::new()];
                        for o in &options {
                            acc = acc.iter().flat_map(|p| o.iter().map(move |&d| [p.clone(), vec![d]].concat())).collect();
                        }
                        for d in acc {
                            rsucc_pool.push(encode(&d, &rsizes) as u16);
                        }
                    }
                    rsucc_off.push(rsucc_pool.len() as u32);
                }
            }
        }
        // ---- underlying mode: registers advanced by each entry's tier ----
        let under = g.options.underlying;
        let mut next_l = Vec::new();
        let mut rinv_off = Vec::new();
        let mut rinv_pool = Vec::new();
        if under && t.n_regs > 1 {
            for e in &g.entries {
                for r in 0..t.n_regs {
                    next_l.push(t.run(r, e.register_text()) as u16);
                }
            }
        }
        if under && rt.n_regs > 1 {
            rinv_off.push(0);
            for e in &g.entries {
                let mut by: Vec<Vec<u16>> = vec![Vec::new(); rt.n_regs as usize];
                for qj in 0..rt.n_regs {
                    by[rt.run_back(qj, e.register_text()) as usize].push(qj as u16);
                }
                for list in by {
                    rinv_pool.extend(list);
                    rinv_off.push(rinv_pool.len() as u32);
                }
            }
        }
        let reads_right: Vec<u8> = if under { g.entries.iter().map(|e| e.reads_right as u8).collect() } else { Vec::new() };
        // ---- INDUCE: exceptions an unknown stem may carry ----
        let mut wild_pins = vec![(0u32, 0u32)];
        if !g.induce_left.is_empty() {
            wild_pins.clear();
            for p in 0..n_pins.max(1) {
                let d = decode(p, &pin_sizes);
                // digit 0 = unpinned; v+1 = pinned to value v (v = 0 is `none`)
                if d.iter().enumerate().all(|(i, &di)| di == 0 || (di >= 2 && g.induce_left.contains(&(i as u16)))) {
                    wild_pins.push((p, d.iter().filter(|&&x| x > 0).count() as u32));
                }
            }
        }
        let rpin_wild: Vec<u32> = (0..n_rpins.max(1))
            .map(|p| {
                let d = decode(p, &rsizes);
                let mut exc = 0;
                for (i, &di) in d.iter().enumerate() {
                    if di >= 2 {
                        // digit 2 = pinned to `none`: never an exception
                        if di == 2 || !g.induce_right.contains(&(i as u16)) {
                            return u32::MAX;
                        }
                        exc += 1;
                    }
                }
                exc
            })
            .collect();
        let feats = |list: &[u16], tr: &Tracker| -> Vec<(String, Vec<String>)> {
            list.iter().map(|&f| (tr.feat_names[f as usize].clone(), tr.feat_values[f as usize].clone())).collect()
        };
        let pin_feats = feats(&g.pinnable, t);
        let rpin_feats = feats(&g.rpinnable, rt);
        let guard_words = ((t.n_regs * rt.n_regs) as usize).div_ceil(64) as u32;
        let guard_pool: Vec<u64> = g.guards.iter().skip(1).flat_map(|b| b.iter().copied()).collect();
        let mut mt = Morphotactics {
            classes: g.classes.iter().map(|c| ClassRt { name: c.name.clone(), open_tag: c.open_tag.clone() }).collect(),
            entries: g
                .entries
                .iter()
                .map(|e| EntryRt {
                    lexicon: e.lexicon,
                    cont: e.cont,
                    filter: e.filter,
                    lex_id: e.lex_id,
                    guard: e.guard,
                    surface: e.surface.clone(),
                    lemma: e.lemma.clone(),
                    tag: e.tag.clone(),
                    zeros: e.zeros.clone(),
                })
                .collect(),
            filters: g.filters.clone(),
            cc_off,
            cc_pool,
            start: Vec::new(),
            wild_off: vec![0],
            wild_pool: Vec::new(),
            max_morph_chars: g.options.max_morph_chars as u32,
            fb: bits(g.filters.len()),
            pb: bits(n_pins as usize),
            rb: bits(n_rpins as usize),
            under,
            lb: if under { bits(t.n_regs as usize) } else { 0 },
            qb: if under && rt.n_regs > 1 { bits(rt.n_regs as usize + 1) } else { 0 },
            sh: 0,
            fsh: 0,
            psh: 0,
            rsh: 0,
            lsh: 0,
            fmask: 0,
            pmask: 0,
            rmask: 0,
            lmask: 0,
            qmask: 0,
            ctx: false,
            multi: false,
            rn: 1,
            rpins_on: false,
            left: RegTable::from_tracker(t),
            right: RegTable::from_tracker(rt),
            guard_words,
            guard_pool,
            n_pins,
            pin_apply,
            pin_next,
            n_rpins,
            rpin_apply,
            rpin_free,
            rsucc_off,
            rsucc_pool,
            next_l,
            rinv_off,
            rinv_pool,
            reads_right,
            wild_pins,
            rpin_wild,
            pin_feats,
            rpin_feats,
            norm: NormOptions {
                case: g.options.case,
                hangul_jamo: g.options.hangul_jamo,
                apostrophe_in_words: g.options.apostrophe_in_words,
                strip_tones: g.options.strip_tones,
                strip_vowel_marks: g.options.strip_vowel_marks,
            },
            derivation: g.options.derivation.clone(),
            context: g.options.context.clone(),
            shapes: Vec::new(),
            shape_pre: Vec::new(),
            lex_sorted: false,
            lex_of: Vec::new(),
        };
        // `'form'` selectors meet the normalized word text.
        for rule in &mut mt.context {
            for s in rule.prefer.iter_mut().chain(&mut rule.over).chain(&mut rule.near) {
                if let crate::spec::Sel::Form(f) = s {
                    *f = crate::normalize::normalize(f, &mt.norm);
                }
            }
        }
        mt.derive();
        mt.start = mt.closure(mt.state(g.root, 0, 0)).collect();
        for c in 0..nc {
            let mut out = std::collections::BTreeSet::new();
            if g.classes[c].open_tag.is_some() {
                for e in g.entries.iter().filter(|e| e.lexicon as usize == c && e.rule.is_none()) {
                    out.extend(mt.closure(mt.state(e.cont, 0, 0)));
                }
            }
            mt.wild_pool.extend(out);
            mt.wild_off.push(mt.wild_pool.len() as u32);
        }
        mt.shapes = shapes::derive(g);
        mt.shape_pre = shapes::prefixes(&mt.shapes);
        mt
    }

    // State layout, low → high bits: right register (qb), left register
    // (lb), right pins (rb), left pins (pb), filter (fb), class. In surface
    // mode qb = lb = 0, so numbering is exactly the old (class|filter|pin|rpin).
    fn derive(&mut self) {
        self.lex_of = self.entries.iter().map(|e| e.lexicon).collect();
        self.lex_sorted = self.lex_of.windows(2).all(|w| w[0] <= w[1]);
        self.lsh = self.qb;
        self.rsh = self.qb + self.lb;
        self.psh = self.rsh + self.rb;
        self.fsh = self.psh + self.pb;
        self.sh = self.fsh + self.fb;
        self.fmask = (1u32 << self.fb) - 1;
        self.pmask = (1u32 << self.pb) - 1;
        self.rmask = (1u32 << self.rb) - 1;
        self.lmask = (1u32 << self.lb) - 1;
        self.qmask = (1u32 << self.qb) - 1;
        self.ctx = self.left.n_regs > 1 || self.n_pins > 1 || self.right.n_regs > 1 || self.n_rpins > 1;
        self.rn = self.right.n_regs;
        self.rpins_on = self.n_rpins > 1;
        self.multi = self.rpins_on || self.under;
    }

    /// Bits used by a state — must be ≤ 32.
    pub fn state_bits(&self) -> u32 {
        bits(self.classes.len()) + self.fb + self.pb + self.rb + self.lb + self.qb
    }

    /// State with right pins ⊤, left register `none`, right register ⊤.
    #[inline(always)]
    pub fn state(&self, class: ClassId, filter: u16, pin: u32) -> u32 {
        ((class as u32) << self.sh) | ((filter as u32) << self.fsh) | (pin << self.psh)
    }
    #[inline(always)]
    fn state_full(&self, class: ClassId, filter: u16, pin: u32, rpin: u32, l: u32, qcode: u32) -> u32 {
        self.state(class, filter, pin) | (rpin << self.rsh) | (l << self.lsh) | qcode
    }
    #[inline(always)]
    fn lreg_of(&self, s: u32) -> u32 {
        (s >> self.lsh) & self.lmask
    }
    /// Right register code in the state: 0 = ⊤, q + 1 = q.
    #[inline(always)]
    fn rreg_code(&self, s: u32) -> u32 {
        s & self.qmask
    }
    /// Underlying mode: registers live in the state.
    #[inline]
    pub fn underlying(&self) -> bool {
        self.under
    }
    /// Transitions may produce several targets (use [`Self::step_multi`]).
    #[inline(always)]
    pub fn needs_multi(&self) -> bool {
        self.multi
    }
    #[inline(always)]
    pub fn class_of(&self, s: u32) -> ClassId {
        (s >> self.sh) as ClassId
    }
    #[inline(always)]
    pub fn filter_of(&self, s: u32) -> u16 {
        ((s >> self.fsh) & self.fmask) as u16
    }
    #[inline(always)]
    pub fn pin_of(&self, s: u32) -> u32 {
        (s >> self.psh) & self.pmask
    }
    #[inline(always)]
    pub fn rpin_of(&self, s: u32) -> u32 {
        (s >> self.rsh) & self.rmask
    }

    /// Epsilon closure of a state (filter and pins carried unchanged).
    pub fn closure(&self, s: u32) -> impl Iterator<Item = u32> + '_ {
        let c = self.class_of(s) as usize;
        let sh = self.sh;
        let low = s & ((1u32 << sh) - 1);
        self.cc_pool[self.cc_off[c] as usize..self.cc_off[c + 1] as usize].iter().map(move |&k| (k << sh) | low)
    }

    pub fn start_states(&self) -> &[u32] {
        &self.start
    }

    /// Is the left register computed from the surface text (per word)?
    #[inline]
    pub fn has_register(&self) -> bool {
        self.left.n_regs > 1 && !self.under
    }

    /// Is the right register computed from the surface text (per word)?
    #[inline]
    pub fn has_right_register(&self) -> bool {
        self.right.n_regs > 1 && !self.under
    }

    /// Are there right pins (multi-target transitions)?
    #[inline(always)]
    pub fn has_rpins(&self) -> bool {
        self.rpins_on
    }

    /// Left register at every byte offset of `word` (only char boundaries
    /// are meaningful). Empty if no left register.
    pub fn track(&self, word: &str, out: &mut Vec<u16>) {
        out.clear();
        if !self.has_register() {
            return;
        }
        out.resize(word.len() + 1, 0);
        let mut r = 0u32;
        for (i, c) in word.char_indices() {
            r = self.left.step(r, c);
            for b in &mut out[i + 1..=i + c.len_utf8()] {
                *b = r as u16;
            }
        }
    }

    /// Right register at every byte offset: a function of the text after
    /// it, computed in one backward pass. Empty if no right register.
    pub fn track_right(&self, word: &str, out: &mut Vec<u16>) {
        out.clear();
        if !self.has_right_register() {
            return;
        }
        out.resize(word.len() + 1, 0);
        let mut q = 0u32;
        for (i, c) in word.char_indices().rev() {
            q = self.right.step(q, c);
            for b in &mut out[i..i + c.len_utf8()] {
                *b = q as u16;
            }
        }
    }

    /// Consume morpheme `entry` from state `s`, with left register `rl` at
    /// its start and right register `rr` at its end (pre-closure target).
    /// Only valid when the language has no right pins ([`Self::step_multi`]).
    #[inline(always)]
    pub fn step(&self, s: u32, entry: u32, rl: u32, rr: u32) -> Option<u32> {
        let e = &self.entries[entry as usize];
        if self.class_of(s) != e.lexicon || !self.filters[self.filter_of(s) as usize].permits(e.lex_id) {
            return None;
        }
        if !self.ctx {
            // No context registers (e.g. English): exactly the old fast path.
            return Some(self.state(e.cont, e.filter, 0));
        }
        self.step_ctx(s, entry, rl, rr)
    }

    #[inline(always)]
    fn guard_ok(&self, guard: u32, eff_l: u32, eff_r: u32) -> bool {
        if guard == 0 {
            return true;
        }
        let i = eff_l * self.rn + eff_r;
        let w = self.guard_pool[(guard - 1) as usize * self.guard_words as usize + (i / 64) as usize];
        w >> (i % 64) & 1 == 1
    }

    #[inline(always)]
    fn eff_left(&self, s: u32, rl: u32) -> u32 {
        if self.n_pins > 1 {
            self.pin_apply[self.pin_of(s) as usize * self.left.n_regs as usize + rl as usize] as u32
        } else {
            rl
        }
    }

    #[inline(always)]
    fn next_pin(&self, s: u32, entry: u32) -> u32 {
        if self.n_pins > 1 {
            self.pin_next[entry as usize * self.n_pins as usize + self.pin_of(s) as usize] as u32
        } else {
            0
        }
    }

    /// Guard check and left-pin update, for languages with registers.
    #[inline(never)]
    fn step_ctx(&self, s: u32, entry: u32, rl: u32, rr: u32) -> Option<u32> {
        let e = &self.entries[entry as usize];
        if e.guard != 0 && !self.guard_ok(e.guard, self.eff_left(s, rl), rr) {
            return None;
        }
        Some(self.state(e.cont, e.filter, self.next_pin(s, entry)))
    }

    /// Like [`Self::step`], for languages with right pins or underlying
    /// registers: pushes every target whose guesses are consistent and whose
    /// guard holds. `rl`/`rr` are the surface registers (ignored in
    /// underlying mode, where the state carries them).
    pub fn step_multi(&self, s: u32, entry: u32, rl: u32, rr: u32, out: &mut Vec<u32>) {
        let e = &self.entries[entry as usize];
        if self.class_of(s) != e.lexicon || !self.filters[self.filter_of(s) as usize].permits(e.lex_id) {
            return;
        }
        let (l_true, new_l) = if self.under {
            let l = self.lreg_of(s);
            let nl = if self.left.n_regs > 1 { self.next_l[entry as usize * self.left.n_regs as usize + l as usize] as u32 } else { 0 };
            (l, nl)
        } else {
            (rl, 0)
        };
        let eff_l = self.eff_left(s, l_true);
        let pin = self.next_pin(s, entry);
        // right register candidates at the entry's end: (state code, value)
        let single;
        let many: Vec<(u32, u32)>;
        let cands: &[(u32, u32)] = if !self.under {
            single = [(0, rr)];
            &single
        } else if self.right.n_regs == 1 {
            single = [(0, 0)];
            &single
        } else {
            let code = self.rreg_code(s);
            if code == 0 {
                if self.reads_right[entry as usize] == 1 {
                    many = (0..self.right.n_regs).map(|q| (q + 1, q)).collect();
                    &many
                } else {
                    single = [(0, 0)];
                    &single
                }
            } else {
                let k = entry as usize * self.right.n_regs as usize + (code - 1) as usize;
                many = self.rinv_pool[self.rinv_off[k] as usize..self.rinv_off[k + 1] as usize].iter().map(|&q| (q as u32 + 1, q as u32)).collect();
                &many
            }
        };
        let k = entry as usize * self.n_rpins as usize + self.rpin_of(s) as usize;
        let succ: &[u16] = if self.rpins_on { &self.rsucc_pool[self.rsucc_off[k] as usize..self.rsucc_off[k + 1] as usize] } else { &[0] };
        for &(qcode, qval) in cands {
            for &rp in succ {
                let eff_r = if self.rpins_on { self.rpin_apply[rp as usize * self.right.n_regs as usize + qval as usize] as u32 } else { qval };
                if self.guard_ok(e.guard, eff_l, eff_r) {
                    out.push(self.state_full(e.cont, e.filter, pin, rp as u32, new_l, qcode));
                }
            }
        }
    }

    #[inline]
    pub fn accepting(&self, s: u32) -> bool {
        self.class_of(s) == END
            && self.filters[self.filter_of(s) as usize].permits_unknown()
            && (!self.rpins_on || self.rpin_free[self.rpin_of(s) as usize])
            && self.rreg_code(s) <= 1
    }

    /// States entered after an unknown stem `span` posited in `s`'s (open)
    /// class, each with the number of exceptions (INDUCE values) the stem
    /// has to carry. Without INDUCE an unknown stem clears all pins.
    pub fn wildcard_into(&self, s: u32, span: &str, out: &mut Vec<(u32, u32)>) {
        let c = self.class_of(s) as usize;
        if self.classes[c].open_tag.is_none() || !self.filters[self.filter_of(s) as usize].permits_unknown() {
            return;
        }
        let rexc = if self.rpins_on { self.rpin_wild[self.rpin_of(s) as usize] } else { 0 };
        if rexc == u32::MAX {
            return;
        }
        let base = &self.wild_pool[self.wild_off[c] as usize..self.wild_off[c + 1] as usize];
        if !self.under {
            for &w in base {
                for &(pin, exc) in &self.wild_pins {
                    out.push((w | (pin << self.psh), exc + rexc));
                }
            }
            return;
        }
        // underlying mode: the registers after the stem come from its text
        let mut l = self.lreg_of(s);
        if self.left.n_regs > 1 {
            for ch in span.chars() {
                l = self.left.step(l, ch);
            }
        }
        let code = self.rreg_code(s);
        let any_q = self.right.n_regs == 1 || code == 0;
        for qj in 0..if any_q { 1 } else { self.right.n_regs } {
            let qc = if any_q {
                0
            } else if span.chars().rev().fold(qj, |q, ch| self.right.step(q, ch)) == code - 1 {
                qj + 1
            } else {
                continue;
            };
            for &w in base {
                for &(pin, exc) in &self.wild_pins {
                    out.push((w | (pin << self.psh) | (l << self.lsh) | qc, exc + rexc));
                }
            }
        }
    }

    /// Positions in `payload` (sorted entry ids) of the entries of lexicon
    /// `class`, when entry ids are grouped by lexicon (as compiled grammars
    /// are); `None` otherwise.
    pub fn lexicon_range(&self, payload: &[u32], class: ClassId) -> Option<std::ops::Range<usize>> {
        if !self.lex_sorted {
            return None;
        }
        let lex = |e: &u32| self.lex_of.get(*e as usize).copied().unwrap_or(ClassId::MAX);
        Some(payload.partition_point(|e| lex(e) < class)..payload.partition_point(|e| lex(e) <= class))
    }

    /// May an unknown stem start in state `s` (open class whose filter
    /// admits it)?
    pub fn wildcard_allowed(&self, s: u32) -> bool {
        let open = self.classes.get(self.class_of(s) as usize).is_some_and(|c| c.open_tag.is_some());
        open && self.filters.get(self.filter_of(s) as usize).is_some_and(Filter::permits_unknown)
    }

    /// Exceptions an unknown stem carries between the state `before` it
    /// (right pins guessed by its left side) and the state `after` it (left
    /// pins it set), e.g. `vow=bu`.
    pub fn describe_exceptions(&self, before: u32, after: u32) -> String {
        let digits = |mut code: u32, feats: &[(String, Vec<String>)], extra: u32| -> Vec<String> {
            let mut v = Vec::new();
            for (n, vs) in feats {
                let size = vs.len() as u32 + extra;
                let d = code % size;
                code /= size;
                if d >= extra {
                    v.push(format!("{n}={}", vs[(d - extra) as usize]));
                }
            }
            v
        };
        let mut parts = Vec::new();
        if self.n_pins > 1 {
            parts.extend(digits(self.pin_of(after), &self.pin_feats, 1));
        }
        if self.rpins_on {
            parts.extend(digits(self.rpin_of(before), &self.rpin_feats, 2));
        }
        parts.join(" ")
    }

    pub fn open_tag(&self, s: u32) -> &str {
        self.classes[self.class_of(s) as usize].open_tag.as_deref().unwrap_or("?")
    }

    // ---- serialization ---------------------------------------------------

    pub fn write(&self, w: &mut Writer) {
        w.u32(self.classes.len() as u32);
        for c in &self.classes {
            w.str(&c.name);
            match &c.open_tag {
                Some(t) => {
                    w.u8(1);
                    w.str(t);
                }
                None => w.u8(0),
            }
        }
        w.u32(self.filters.len() as u32);
        for f in &self.filters {
            w.u8(f.kind as u8);
            w.u32s(&f.ids);
        }
        w.u32(self.entries.len() as u32);
        for e in &self.entries {
            w.u32(e.lexicon as u32);
            w.u32(e.cont as u32);
            w.u32(e.filter as u32);
            w.u32(e.lex_id);
            w.u32(e.guard);
            w.str(&e.surface);
            w.str(&e.lemma);
            w.str(&e.tag);
            w.u32(e.zeros.len() as u32);
            for z in &e.zeros {
                w.str(z);
            }
        }
        w.u32s(&self.cc_off);
        w.u32s(&self.cc_pool);
        w.u32s(&self.start);
        w.u32s(&self.wild_off);
        w.u32s(&self.wild_pool);
        w.u32(self.max_morph_chars);
        self.left.write(w);
        self.right.write(w);
        w.u32(self.guard_words);
        w.u64s(&self.guard_pool);
        w.u32(self.n_pins);
        w.u32s(&self.pin_apply.iter().map(|&x| x as u32).collect::<Vec<_>>());
        w.u32s(&self.pin_next.iter().map(|&x| x as u32).collect::<Vec<_>>());
        w.u32(self.n_rpins);
        w.u32s(&self.rpin_apply.iter().map(|&x| x as u32).collect::<Vec<_>>());
        w.u32s(&self.rpin_free.iter().map(|&x| x as u32).collect::<Vec<_>>());
        w.u32s(&self.rsucc_off);
        w.u32s(&self.rsucc_pool.iter().map(|&x| x as u32).collect::<Vec<_>>());
        w.u8(self.under as u8);
        w.u32s(&self.next_l.iter().map(|&x| x as u32).collect::<Vec<_>>());
        w.u32s(&self.rinv_off);
        w.u32s(&self.rinv_pool.iter().map(|&x| x as u32).collect::<Vec<_>>());
        w.u32s(&self.reads_right.iter().map(|&x| x as u32).collect::<Vec<_>>());
        w.u32s(&self.wild_pins.iter().map(|p| p.0).collect::<Vec<_>>());
        w.u32s(&self.wild_pins.iter().map(|p| p.1).collect::<Vec<_>>());
        w.u32s(&self.rpin_wild);
        for feats in [&self.pin_feats, &self.rpin_feats] {
            w.u32(feats.len() as u32);
            for (n, vs) in feats.iter() {
                w.str(n);
                w.u32(vs.len() as u32);
                for v in vs {
                    w.str(v);
                }
            }
        }
        w.u8(match self.norm.case {
            CaseMode::Lower => 0,
            CaseMode::Turkish => 1,
            CaseMode::None => 2,
        });
        w.u8(self.norm.hangul_jamo as u8);
        w.u8(self.norm.apostrophe_in_words as u8);
        w.u8(self.norm.strip_tones as u8);
        w.u8(self.norm.strip_vowel_marks as u8);
        w.u32(self.derivation.len() as u32);
        for d in &self.derivation {
            w.str(d);
        }
        write_context(&self.context, w);
        shapes::write(&self.shapes, w);
    }

    pub fn read(r: &mut Reader) -> Result<Self> {
        let nc = r.count(5)?;
        let mut classes = Vec::with_capacity(nc);
        for _ in 0..nc {
            let name = r.string()?;
            let open_tag = match r.u8()? {
                0 => None,
                1 => Some(r.string()?),
                _ => return art_err("bad open-tag flag"),
            };
            classes.push(ClassRt { name, open_tag });
        }
        let nf = r.count(5)?;
        let mut filters = Vec::with_capacity(nf);
        for _ in 0..nf {
            let kind = match r.u8()? {
                0 => FilterKind::None,
                1 => FilterKind::Allow,
                2 => FilterKind::Deny,
                _ => return art_err("bad filter kind"),
            };
            let ids = r.u32s()?;
            if ids.windows(2).any(|w| w[0] >= w[1]) {
                return art_err("filter ids not strictly sorted");
            }
            filters.push(Filter { kind, ids });
        }
        let ne = r.count(36)?;
        let mut entries = Vec::with_capacity(ne);
        for _ in 0..ne {
            let lexicon = r.u32()?;
            let cont = r.u32()?;
            let filter = r.u32()?;
            let lex_id = r.u32()?;
            let guard = r.u32()?;
            let surface = r.string()?;
            let lemma = r.string()?;
            let tag = r.string()?;
            let nz = r.count(4)?;
            let mut zeros = Vec::with_capacity(nz);
            for _ in 0..nz {
                zeros.push(r.string()?);
            }
            if lexicon as usize >= nc || lexicon == END as u32 || cont as usize >= nc || filter as usize >= nf {
                return art_err("entry references unknown class/filter");
            }
            if surface.is_empty() {
                return art_err("entry with empty surface");
            }
            entries.push(EntryRt { lexicon: lexicon as u16, cont: cont as u16, filter: filter as u16, lex_id, guard, surface, lemma, tag, zeros });
        }
        let cc_off = r.u32s()?;
        let cc_pool = r.u32s()?;
        let start = r.u32s()?;
        let wild_off = r.u32s()?;
        let wild_pool = r.u32s()?;
        let max_morph_chars = r.u32()?;
        let left = RegTable::read(r)?;
        let right = RegTable::read(r)?;
        let guard_words = r.u32()?;
        let guard_pool = r.u64s()?;
        let n_pins = r.u32()?;
        let narrow = |v: Vec<u32>| -> Vec<u16> { v.into_iter().map(|x| x.min(u16::MAX as u32) as u16).collect() };
        let pin_apply = narrow(r.u32s()?);
        let pin_next = narrow(r.u32s()?);
        let n_rpins = r.u32()?;
        let rpin_apply = narrow(r.u32s()?);
        let rpin_free_raw = r.u32s()?;
        let rsucc_off = r.u32s()?;
        let rsucc_pool = narrow(r.u32s()?);
        let under = match r.u8()? {
            0 => false,
            1 => true,
            _ => return art_err("bad harmony mode"),
        };
        let next_l = narrow(r.u32s()?);
        let rinv_off = r.u32s()?;
        let rinv_pool = narrow(r.u32s()?);
        let reads_right: Vec<u8> = r.u32s()?.into_iter().map(|x| x.min(2) as u8).collect();
        let wp_codes = r.u32s()?;
        let wp_exc = r.u32s()?;
        let rpin_wild = r.u32s()?;
        let read_feats = |r: &mut Reader| -> Result<Vec<(String, Vec<String>)>> {
            let n = r.count(8)?;
            let mut feats = Vec::with_capacity(n);
            for _ in 0..n {
                let name = r.string()?;
                let nv = r.count(4)?;
                let mut vs = Vec::with_capacity(nv);
                for _ in 0..nv {
                    vs.push(r.string()?);
                }
                feats.push((name, vs));
            }
            Ok(feats)
        };
        let pin_feats = read_feats(r)?;
        let rpin_feats = read_feats(r)?;
        let case = match r.u8()? {
            0 => CaseMode::Lower,
            1 => CaseMode::Turkish,
            2 => CaseMode::None,
            _ => return art_err("bad case mode"),
        };
        let hangul_jamo = r.u8()? == 1;
        let apostrophe_in_words = r.u8()? == 1;
        let strip_tones = r.u8()? == 1;
        let strip_vowel_marks = r.u8()? == 1;
        let n_derivation = r.count(4)?;
        let mut derivation = Vec::with_capacity(n_derivation);
        for _ in 0..n_derivation {
            derivation.push(r.string()?);
        }
        let context = read_context(r)?;
        let shapes = shapes::read(r, &classes, &entries)?;

        // ---- validation ----
        let max_pins = crate::spec::expand::MAX_PINS;
        if nc < 2 || nf == 0 || nc > u16::MAX as usize || nf > u16::MAX as usize {
            return art_err("bad class/filter counts");
        }
        if n_pins == 0 || n_pins > max_pins || n_rpins == 0 || n_rpins > max_pins {
            return art_err("bad pin sizes");
        }
        let pairs = left.n_regs as u64 * right.n_regs as u64;
        if pairs > crate::spec::expand::MAX_REGS as u64 * 4 {
            return art_err("register pair space too large");
        }
        if guard_words as u64 != pairs.div_ceil(64) || guard_pool.len() % guard_words.max(1) as usize != 0 {
            return art_err("bad guard pool");
        }
        let n_guards = guard_pool.len() as u64 / guard_words.max(1) as u64;
        if entries.iter().any(|e| e.guard as u64 > n_guards) {
            return art_err("entry guard out of range");
        }
        if n_pins > 1 {
            if pin_apply.len() as u64 != n_pins as u64 * left.n_regs as u64 || pin_apply.iter().any(|&x| x as u32 >= left.n_regs) {
                return art_err("bad pin table");
            }
            if pin_next.len() as u64 != n_pins as u64 * ne as u64 || pin_next.iter().any(|&x| x as u32 >= n_pins) {
                return art_err("bad pin transitions");
            }
        } else if !pin_apply.is_empty() || !pin_next.is_empty() {
            return art_err("unexpected pin tables");
        }
        let rpin_free: Vec<bool> = rpin_free_raw.iter().map(|&x| x == 1).collect();
        if rpin_free_raw.iter().any(|&x| x > 1) || rpin_free.len() as u32 != n_rpins.max(1) {
            return art_err("bad right-pin acceptance table");
        }
        if n_rpins > 1 {
            if rpin_apply.len() as u64 != n_rpins as u64 * right.n_regs as u64 || rpin_apply.iter().any(|&x| x as u32 >= right.n_regs) {
                return art_err("bad right-pin table");
            }
            check_csr(&rsucc_off, rsucc_pool.len(), ne * n_rpins as usize)?;
            if rsucc_pool.iter().any(|&x| x as u32 >= n_rpins) {
                return art_err("bad right-pin transitions");
            }
        } else if !rpin_apply.is_empty() || !rsucc_off.is_empty() || !rsucc_pool.is_empty() {
            return art_err("unexpected right-pin tables");
        }
        let (fb, pb, rb) = (bits(nf), bits(n_pins as usize), bits(n_rpins as usize));
        let lb = if under { bits(left.n_regs as usize) } else { 0 };
        let qb = if under && right.n_regs > 1 { bits(right.n_regs as usize + 1) } else { 0 };
        if bits(nc) + fb + pb + rb + lb + qb > 32 {
            return art_err("state space exceeds 32 bits");
        }
        // underlying-mode tables
        if under && left.n_regs > 1 {
            if next_l.len() as u64 != ne as u64 * left.n_regs as u64 || next_l.iter().any(|&x| x as u32 >= left.n_regs) {
                return art_err("bad left register transitions");
            }
        } else if !next_l.is_empty() {
            return art_err("unexpected left register transitions");
        }
        if under && right.n_regs > 1 {
            check_csr(&rinv_off, rinv_pool.len(), ne * right.n_regs as usize)?;
            if rinv_pool.iter().any(|&x| x as u32 >= right.n_regs) {
                return art_err("bad right register inverse table");
            }
        } else if !rinv_off.is_empty() || !rinv_pool.is_empty() {
            return art_err("unexpected right register inverse table");
        }
        if (under && reads_right.len() != ne) || (!under && !reads_right.is_empty()) || reads_right.iter().any(|&x| x > 1) {
            return art_err("bad reads-right table");
        }
        if wp_codes.len() != wp_exc.len() || wp_codes.is_empty() || wp_codes.iter().any(|&p| p >= n_pins.max(1)) {
            return art_err("bad wildcard pin table");
        }
        let wild_pins: Vec<(u32, u32)> = wp_codes.into_iter().zip(wp_exc).collect();
        if rpin_wild.len() as u32 != n_rpins.max(1) {
            return art_err("bad wildcard right-pin table");
        }
        let size_ok = |feats: &[(String, Vec<String>)], extra: u32, n: u32| feats.iter().map(|(_, v)| v.len() as u64 + extra as u64).product::<u64>() == n as u64;
        if !size_ok(&pin_feats, 1, n_pins) || !size_ok(&rpin_feats, 2, n_rpins) {
            return art_err("pin feature table does not match pin space");
        }
        check_csr(&cc_off, cc_pool.len(), nc)?;
        check_csr(&wild_off, wild_pool.len(), nc)?;
        if cc_pool.iter().any(|&c| c as usize >= nc) {
            return art_err("closure references unknown class");
        }
        let mut mt = Morphotactics {
            classes,
            entries,
            filters,
            cc_off,
            cc_pool,
            start,
            wild_off,
            wild_pool,
            max_morph_chars,
            fb,
            pb,
            rb,
            under,
            lb,
            qb,
            sh: 0,
            fsh: 0,
            psh: 0,
            rsh: 0,
            lsh: 0,
            fmask: 0,
            pmask: 0,
            rmask: 0,
            lmask: 0,
            qmask: 0,
            ctx: false,
            multi: false,
            rn: 1,
            rpins_on: false,
            left,
            right,
            guard_words,
            guard_pool,
            n_pins,
            pin_apply,
            pin_next,
            n_rpins,
            rpin_apply,
            rpin_free,
            rsucc_off,
            rsucc_pool,
            next_l,
            rinv_off,
            rinv_pool,
            reads_right,
            wild_pins,
            rpin_wild,
            pin_feats,
            rpin_feats,
            norm: NormOptions { case, hangul_jamo, apostrophe_in_words, strip_tones, strip_vowel_marks },
            derivation,
            context,
            shape_pre: shapes::prefixes(&shapes),
            shapes,
            lex_sorted: false,
            lex_of: Vec::new(),
        };
        mt.derive();
        let valid = |s: &u32| {
            (*s >> mt.sh) < nc as u32
                && (mt.filter_of(*s) as usize) < nf
                && mt.pin_of(*s) < n_pins
                && mt.rpin_of(*s) < n_rpins
                && mt.lreg_of(*s) < mt.left.n_regs
                && mt.rreg_code(*s) <= mt.right.n_regs
        };
        if !mt.start.iter().all(valid) || !mt.wild_pool.iter().all(valid) {
            return art_err("state out of range");
        }
        Ok(mt)
    }
}

fn decode(p: u32, sizes: &[u32]) -> Vec<u32> {
    let mut d = Vec::with_capacity(sizes.len());
    let mut x = p;
    for s in sizes {
        d.push(x % s);
        x /= s;
    }
    d
}

fn encode(d: &[u32], sizes: &[u32]) -> u32 {
    let mut p = 0;
    for i in (0..d.len()).rev() {
        p = p * sizes[i] + d[i];
    }
    p
}

/// Register `r` with feature `f` set to value `v`.
fn with_value(t: &Tracker, r: u32, f: usize, v: u32) -> u32 {
    let n = t.feat_values[f].len() as u32;
    let cur = (r / t.strides[f]) % n;
    r - cur * t.strides[f] + v * t.strides[f]
}

/// Validate a CSR offset array: `n+1` monotone offsets ending at `pool_len`.
pub(crate) fn check_csr(off: &[u32], pool_len: usize, n: usize) -> Result<()> {
    if off.len() != n + 1 || off.first() != Some(&0) || off.last().map(|&x| x as usize) != Some(pool_len) {
        return art_err("bad CSR offsets");
    }
    if off.windows(2).any(|w| w[0] > w[1]) {
        return art_err("non-monotone CSR offsets");
    }
    Ok(())
}

fn write_sels(sels: &[crate::spec::Sel], w: &mut Writer) {
    w.u32(sels.len() as u32);
    for s in sels {
        let (kind, text) = match s {
            crate::spec::Sel::Tag(t) => (0, t),
            crate::spec::Sel::Lemma(l) => (1, l),
            crate::spec::Sel::Form(f) => (2, f),
        };
        w.u8(kind);
        w.str(text);
    }
}

fn read_sels(r: &mut Reader) -> Result<Vec<crate::spec::Sel>> {
    let n = r.count(5)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let kind = r.u8()?;
        let text = r.string()?;
        out.push(match kind {
            0 => crate::spec::Sel::Tag(text),
            1 => crate::spec::Sel::Lemma(text),
            2 => crate::spec::Sel::Form(text),
            _ => return art_err("bad context selector"),
        });
    }
    Ok(out)
}

fn write_context(rules: &[crate::spec::ContextRule], w: &mut Writer) {
    w.u32(rules.len() as u32);
    for c in rules {
        write_sels(&c.prefer, w);
        write_sels(&c.over, w);
        w.u8(c.before as u8);
        write_sels(&c.near, w);
    }
}

fn read_context(r: &mut Reader) -> Result<Vec<crate::spec::ContextRule>> {
    let n = r.count(13)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let prefer = read_sels(r)?;
        let over = read_sels(r)?;
        let before = match r.u8()? {
            0 => false,
            1 => true,
            _ => return art_err("bad context side"),
        };
        let near = read_sels(r)?;
        out.push(crate::spec::ContextRule { prefer, over, before, near });
    }
    Ok(out)
}
