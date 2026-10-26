//! Validation + compilation of a spec into a [`Grammar`] of concrete entries.
//!
//! Three mechanisms, all resolved at compile time:
//!
//! 1. **Context register** (`FEATURE`s): a deterministic automaton over
//!    surface characters. Its state at byte `p` is a function of the text
//!    before `p` only, so the runtime computes it in one O(n) pass and the
//!    lattice gains no states. This is what carries long-distance context
//!    such as vowel harmony across any number of morphemes.
//! 2. **Archiphonemes** (`{A}`, `{I}`, ...): for every entering register
//!    value, the template is realised left to right (the register evolves
//!    over the realised chars). Equal strings are grouped into *variants*,
//!    each guarded by the set of register values that produce it.
//! 3. **Rules**: applied sequentially (spec order) to each variant, for each
//!    possible next morpheme; optional rules branch. Each distinct result
//!    becomes an allomorph whose filter allows exactly the next morphemes that
//!    produce it; the unrewritten form denies the ones obligatory rules
//!    change. For English-style rules this is identical to the previous
//!    one-rule-per-suffix behaviour.

use std::collections::{BTreeMap, BTreeSet};

use super::ast::*;
use crate::error::{spec_err, Result};

pub type ClassId = u16;
/// The end-of-word pseudo-class `#`.
pub const END: ClassId = 0;
/// Hard caps keeping tables small and state packing within 32 bits.
pub const MAX_REGS: u32 = 4096;
pub const MAX_PINS: u32 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FilterKind {
    None = 0,
    Allow = 1,
    Deny = 2,
}

/// Constraint on the next non-epsilon morpheme (by lexical-template id).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Filter {
    pub kind: FilterKind,
    pub ids: Vec<u32>,
}

impl Filter {
    pub fn none() -> Self {
        Filter { kind: FilterKind::None, ids: vec![] }
    }
    pub fn permits(&self, lex_id: u32) -> bool {
        match self.kind {
            FilterKind::None => true,
            FilterKind::Allow => self.ids.binary_search(&lex_id).is_ok(),
            FilterKind::Deny => self.ids.binary_search(&lex_id).is_err(),
        }
    }
    /// End-of-word or an unknown (wildcard) morpheme.
    pub fn permits_unknown(&self) -> bool {
        self.kind != FilterKind::Allow
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassInfo {
    pub name: String,
    pub open_tag: Option<String>,
}

/// A concrete (fully realised) non-epsilon morpheme.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub lexicon: ClassId,
    /// Realised surface (what appears in text).
    pub surface: String,
    /// Lexical template (`l{A}r`); what filters and right contexts see.
    pub lex_surface: String,
    pub lex_id: u32,
    pub lemma: String,
    pub tag: String,
    pub cont: ClassId,
    pub filter: u16,
    /// Rule(s) that produced this allomorph, joined by `+`.
    pub rule: Option<String>,
    /// Guard id into [`Grammar::guards`]; 0 = any register.
    pub guard: u32,
    /// `@set:feature=value` overrides: (pinnable index, value index).
    pub pin_set: Vec<(u16, u16)>,
    /// Pinnable features this surface touches (bit i = pinnable i).
    pub pin_touch: u32,
    /// `@rset:feature=value` (right-context pins): (rpinnable index, value).
    pub rpin_set: Vec<(u16, u16)>,
    /// Right-pinnable features this surface touches.
    pub rpin_touch: u32,
    /// Does this variant's realisation depend on the right register?
    pub reads_right: bool,
    /// Underlying mode: the text the registers read for this morpheme (its
    /// realisation before rules, or an `@tier=` override). None in surface
    /// mode, where registers read `surface`.
    pub tier: Option<String>,
}

impl Entry {
    /// What the context registers read for this morpheme.
    pub fn register_text(&self) -> &str {
        self.tier.as_deref().unwrap_or(&self.surface)
    }
}

/// The context-register automaton, compiled to a dense table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tracker {
    pub feat_names: Vec<String>,
    /// Per feature: value names, index 0 = `none`.
    pub feat_values: Vec<Vec<String>>,
    pub strides: Vec<u32>,
    pub n_regs: u32,
    /// (char, column) sorted by char; chars not listed use column 0.
    pub columns: Vec<(u32, u16)>,
    pub n_cols: u16,
    /// `trans[r * n_cols + col]`
    pub trans: Vec<u16>,
}

impl Tracker {
    fn trivial() -> Self {
        Tracker { feat_names: vec![], feat_values: vec![], strides: vec![], n_regs: 1, columns: vec![], n_cols: 1, trans: vec![0] }
    }
    pub fn col(&self, c: char) -> u16 {
        match self.columns.binary_search_by_key(&(c as u32), |&(k, _)| k) {
            Ok(i) => self.columns[i].1,
            Err(_) => 0,
        }
    }
    #[inline]
    pub fn step(&self, r: u32, c: char) -> u32 {
        self.trans[(r * self.n_cols as u32 + self.col(c) as u32) as usize] as u32
    }
    pub fn run(&self, mut r: u32, s: &str) -> u32 {
        if self.n_regs == 1 {
            return 0;
        }
        for c in s.chars() {
            r = self.step(r, c);
        }
        r
    }
    pub fn value(&self, r: u32, f: usize) -> usize {
        ((r / self.strides[f]) % self.feat_values[f].len() as u32) as usize
    }
    fn with(&self, r: u32, f: usize, v: usize) -> u32 {
        r - self.value(r, f) as u32 * self.strides[f] + v as u32 * self.strides[f]
    }
    /// Human-readable register, e.g. `vow=bu coda=vl`.
    pub fn describe(&self, r: u32) -> String {
        (0..self.feat_names.len()).map(|f| format!("{}={}", self.feat_names[f], self.feat_values[f][self.value(r, f)])).collect::<Vec<_>>().join(" ")
    }
}

#[derive(Clone, Debug)]
pub struct Grammar {
    pub classes: Vec<ClassInfo>,
    pub root: ClassId,
    pub entries: Vec<Entry>,
    pub epsilons: Vec<(ClassId, ClassId)>,
    pub filters: Vec<Filter>,
    pub lex_surfaces: Vec<String>,
    /// Extra pieces from the PIECES section: (text, is_continuation).
    pub extra_pieces: Vec<(String, bool)>,
    pub options: Options,
    /// Left-context register (FEATUREs).
    pub tracker: Tracker,
    /// Right-context register (RFEATUREs), applied from the word's end.
    pub rtracker: Tracker,
    /// Guard bitsets over (left, right) register pairs, bit `r·R_r + q`;
    /// index 0 = "all" (empty vec).
    pub guards: Vec<Vec<u64>>,
    /// Features that some entry pins with `@set` (indices into tracker).
    pub pinnable: Vec<u16>,
    /// Features that some entry pins with `@rset` (indices into rtracker).
    pub rpinnable: Vec<u16>,
    /// INDUCE: pinnable indices (into `pinnable` / `rpinnable`) whose
    /// values unknown stems may choose, and the lexicons they apply to.
    pub induce_left: Vec<u16>,
    pub induce_right: Vec<u16>,
    pub induce_classes: Vec<ClassId>,
}

impl Grammar {
    /// Classes reachable from `c` by epsilon arcs (including `c`), sorted.
    pub fn class_closure(&self, c: ClassId) -> Vec<ClassId> {
        closure(&self.epsilons, c)
    }
    /// Does guard `g` admit left register `r` with right register `q`?
    pub fn guard_has(&self, g: u32, r: u32, q: u32) -> bool {
        let i = r * self.rtracker.n_regs + q;
        g == 0 || self.guards[g as usize].get(i as usize / 64).is_some_and(|w| w >> (i % 64) & 1 == 1)
    }
}

fn closure(eps: &[(ClassId, ClassId)], c: ClassId) -> Vec<ClassId> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![c];
    while let Some(x) = stack.pop() {
        if seen.insert(x) {
            for &(a, b) in eps {
                if a == x {
                    stack.push(b);
                }
            }
        }
    }
    seen.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Classes
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum ClassRef {
    Set(usize),
    BuiltinC,
    BuiltinV,
}

struct Classes {
    names: BTreeMap<String, usize>,
    chars: Vec<BTreeSet<char>>,
    archis: Vec<BTreeSet<String>>,
}

impl Classes {
    fn build(spec: &Spec) -> Result<Self> {
        let mut c = Classes { names: BTreeMap::new(), chars: vec![], archis: vec![] };
        for d in &spec.classes {
            if c.names.contains_key(&d.name) {
                return spec_err(d.line, format!("duplicate CLASS `{}`", d.name));
            }
            c.names.insert(d.name.clone(), c.chars.len());
            let mut ch = BTreeSet::new();
            let mut ar = BTreeSet::new();
            for m in &d.members {
                match m {
                    Sym::Ch(x) => {
                        ch.insert(*x);
                    }
                    Sym::Archi(a) => {
                        ar.insert(a.clone());
                    }
                }
            }
            c.chars.push(ch);
            c.archis.push(ar);
        }
        Ok(c)
    }
    fn resolve(&self, name: &str, line: usize) -> Result<ClassRef> {
        if let Some(&i) = self.names.get(name) {
            return Ok(ClassRef::Set(i));
        }
        match name {
            "C" => Ok(ClassRef::BuiltinC),
            "V" => Ok(ClassRef::BuiltinV),
            _ => spec_err(line, format!("unknown CLASS `{name}`")),
        }
    }
    fn has_char(&self, r: ClassRef, c: char) -> bool {
        match r {
            ClassRef::Set(i) => self.chars[i].contains(&c),
            ClassRef::BuiltinC => is_consonant(c),
            ClassRef::BuiltinV => is_vowel(c),
        }
    }
    fn has_sym(&self, r: ClassRef, s: &Sym) -> bool {
        match (r, s) {
            (_, Sym::Ch(c)) => self.has_char(r, *c),
            (ClassRef::Set(i), Sym::Archi(a)) => self.archis[i].contains(a),
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
enum AtomM {
    Ch(char),
    Class(ClassRef),
    Any,
}

fn compile_atom(a: &Atom, cls: &Classes, line: usize) -> Result<AtomM> {
    Ok(match a {
        Atom::Ch(c) => AtomM::Ch(*c),
        Atom::Class(n) => AtomM::Class(cls.resolve(n, line)?),
        Atom::Any => AtomM::Any,
    })
}

fn atom_matches(a: &AtomM, cls: &Classes, c: char) -> bool {
    match a {
        AtomM::Ch(x) => *x == c,
        AtomM::Class(r) => cls.has_char(*r, c),
        AtomM::Any => true,
    }
}

// ---------------------------------------------------------------------------
// Context registers + archiphonemes
// ---------------------------------------------------------------------------

/// Condition atom: (is right-register feature, feature index, allowed values).
type CondM = Vec<(bool, usize, Vec<usize>)>;

struct FeatM {
    lines: Vec<(CondM, Vec<AtomM>, usize)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reads {
    /// Left context only (or nothing): resolved left to right.
    Left,
    /// Right context only: resolved right to left.
    Right,
    /// Both directions.
    Both,
}

struct ArchiM {
    lines: Vec<(Option<CondM>, String)>,
    reads: Reads,
}

fn cond_ok(lt: &Tracker, rt: &Tracker, cond: &CondM, r: u32, q: u32) -> bool {
    cond.iter().all(|(right, f, vs)| vs.contains(&if *right { rt.value(q, *f) } else { lt.value(r, *f) }))
}

/// One direction's register: compiled table + feature lines (for `touches`).
struct Dir {
    tracker: Tracker,
    feats: Vec<FeatM>,
    index: BTreeMap<String, usize>,
}

struct Phon {
    left: Dir,
    right: Dir,
    archis: BTreeMap<String, ArchiM>,
}

fn resolve_cond(c: &Cond, left: &(BTreeMap<String, usize>, Vec<Vec<String>>), right: &(BTreeMap<String, usize>, Vec<Vec<String>>), allow: (bool, bool), line: usize) -> Result<CondM> {
    let mut out = Vec::new();
    for (f, vs) in c {
        let (is_right, fi, values) = if let Some(&i) = left.0.get(f) {
            (false, i, &left.1[i])
        } else if let Some(&i) = right.0.get(f) {
            (true, i, &right.1[i])
        } else {
            return spec_err(line, format!("unknown FEATURE `{f}`"));
        };
        if is_right && !allow.1 {
            return spec_err(line, format!("FEATURE lines may not test the right-context RFEATURE `{f}`"));
        }
        if !is_right && !allow.0 {
            return spec_err(line, format!("RFEATURE lines may not test the left-context FEATURE `{f}`"));
        }
        let mut vi = Vec::new();
        for v in vs {
            match values.iter().position(|x| x == v) {
                Some(i) => vi.push(i),
                None => return spec_err(line, format!("feature `{f}` has no value `{v}`")),
            }
        }
        out.push((is_right, fi, vi));
    }
    Ok(out)
}

type Names = (BTreeMap<String, usize>, Vec<Vec<String>>);

fn feature_names(defs: &[&FeatureDef]) -> Result<Names> {
    let mut index = BTreeMap::new();
    let mut values = Vec::new();
    for (i, f) in defs.iter().enumerate() {
        if index.insert(f.name.clone(), i).is_some() {
            return spec_err(f.line, format!("duplicate FEATURE `{}`", f.name));
        }
        let mut vals = vec!["none".to_string()];
        for l in &f.lines {
            if !vals.contains(&l.value) {
                vals.push(l.value.clone());
            }
        }
        values.push(vals);
    }
    Ok((index, values))
}

/// Compile one direction's FEATURE lines into a dense transition table.
/// (For a right register the table is applied to chars from the end.)
fn build_dir(defs: &[&FeatureDef], names: &Names, other: &Names, right: bool, cls: &Classes) -> Result<Dir> {
    let values = &names.1;
    let view = tracker_view(values);
    if view.n_regs as u64 > MAX_REGS as u64 || values.iter().map(|v| v.len() as u64).product::<u64>() > MAX_REGS as u64 {
        return spec_err(0, format!("context register too large (> {MAX_REGS} values); use fewer feature values"));
    }
    let mut feats = Vec::new();
    let mut mentioned: BTreeSet<char> = BTreeSet::new();
    for f in defs {
        let mut lines = Vec::new();
        for l in &f.lines {
            let cond = if right { resolve_cond(&l.cond, other, names, (false, true), l.line)? } else { resolve_cond(&l.cond, names, other, (true, false), l.line)? };
            let mut atoms = Vec::new();
            for a in &l.atoms {
                let m = compile_atom(a, cls, l.line)?;
                match &m {
                    AtomM::Ch(c) => {
                        mentioned.insert(*c);
                    }
                    AtomM::Class(ClassRef::Set(i)) => mentioned.extend(cls.chars[*i].iter().copied()),
                    AtomM::Class(_) => return spec_err(l.line, "FEATURE lines need a declared CLASS (built-in C/V are open-ended)"),
                    AtomM::Any => {}
                }
                atoms.push(m);
            }
            // `values` was built from these same lines, so the value is listed.
            let v = values[names.0[&f.name]].iter().position(|x| *x == l.value).unwrap_or(0);
            lines.push((cond, atoms, v));
        }
        feats.push(FeatM { lines });
    }
    let mut tracker = Tracker::trivial();
    if !defs.is_empty() {
        tracker.feat_names = defs.iter().map(|f| f.name.clone()).collect();
        tracker.feat_values = values.clone();
        tracker.strides = view.strides.clone();
        tracker.n_regs = view.n_regs;
        let n = view.n_regs;
        let step_vec = |c: Option<char>| -> Vec<u16> {
            (0..n)
                .map(|r| {
                    let mut out = r;
                    for (fi, f) in feats.iter().enumerate() {
                        for (cond, atoms, v) in &f.lines {
                            let hit = atoms.iter().any(|a| match c {
                                Some(ch) => atom_matches(a, cls, ch),
                                None => matches!(a, AtomM::Any),
                            });
                            // a direction's lines only test its own features
                            if hit && cond_ok(&view, &view, cond, r, r) {
                                out = view.with(out, fi, *v);
                                break;
                            }
                        }
                    }
                    out as u16
                })
                .collect()
        };
        // Column 0 = chars not mentioned anywhere; group identical columns.
        let mut cols: Vec<Vec<u16>> = vec![step_vec(None)];
        let mut columns = Vec::new();
        for &c in &mentioned {
            let v = step_vec(Some(c));
            let id = match cols.iter().position(|x| *x == v) {
                Some(i) => i,
                None => {
                    cols.push(v);
                    cols.len() - 1
                }
            };
            if id != 0 {
                columns.push((c as u32, id as u16));
            }
        }
        if cols.len() as u64 * n as u64 > (1 << 22) {
            return spec_err(0, "context-register table too large");
        }
        tracker.n_cols = cols.len() as u16;
        tracker.trans = vec![0; (n as usize) * cols.len()];
        for (ci, col) in cols.iter().enumerate() {
            for (r, &v) in col.iter().enumerate() {
                tracker.trans[r * cols.len() + ci] = v;
            }
        }
        tracker.columns = columns;
    }
    Ok(Dir { tracker, feats, index: names.0.clone() })
}

fn build_phon(spec: &Spec, cls: &Classes) -> Result<Phon> {
    let ldefs: Vec<&FeatureDef> = spec.features.iter().filter(|f| !f.right).collect();
    let rdefs: Vec<&FeatureDef> = spec.features.iter().filter(|f| f.right).collect();
    let lnames = feature_names(&ldefs)?;
    let rnames = feature_names(&rdefs)?;
    if let Some(f) = rdefs.iter().find(|f| lnames.0.contains_key(&f.name)) {
        return spec_err(f.line, format!("`{}` is both a FEATURE and an RFEATURE", f.name));
    }
    let left = build_dir(&ldefs, &lnames, &rnames, false, cls)?;
    let right = build_dir(&rdefs, &rnames, &lnames, true, cls)?;
    if left.tracker.n_regs as u64 * right.tracker.n_regs as u64 > MAX_REGS as u64 * 4 {
        return spec_err(0, "left × right context registers too large");
    }
    let mut archis = BTreeMap::new();
    for a in &spec.archis {
        let mut lines = Vec::new();
        let (mut rl, mut rr) = (false, false);
        for l in &a.lines {
            let cond = match &l.cond {
                None => None,
                Some(c) => {
                    let m = resolve_cond(c, &lnames, &rnames, (true, true), l.line)?;
                    rl |= m.iter().any(|x| !x.0);
                    rr |= m.iter().any(|x| x.0);
                    Some(m)
                }
            };
            lines.push((cond, l.output.clone()));
        }
        // Totality: some line must fire for every (left, right) register pair.
        for r in 0..left.tracker.n_regs {
            for q in 0..right.tracker.n_regs {
                if !lines.iter().any(|(c, _)| c.as_ref().is_none_or(|c| cond_ok(&left.tracker, &right.tracker, c, r, q))) {
                    let d = [left.tracker.describe(r), right.tracker.describe(q)].join(" ");
                    return spec_err(a.line, format!("ARCHI `{}` has no realisation for {}", a.name, d.trim()));
                }
            }
        }
        let reads = match (rl, rr) {
            (_, false) => Reads::Left,
            (false, true) => Reads::Right,
            (true, true) => Reads::Both,
        };
        if archis.insert(a.name.clone(), ArchiM { lines, reads }).is_some() {
            return spec_err(a.line, format!("duplicate ARCHI `{}`", a.name));
        }
    }
    Ok(Phon { left, right, archis })
}

/// A value-only Tracker (no table) for evaluating conditions while the
/// real table is being built.
fn tracker_view(values: &[Vec<String>]) -> Tracker {
    let mut strides = Vec::new();
    let mut acc = 1u32;
    for v in values {
        strides.push(acc);
        acc = acc.saturating_mul(v.len() as u32);
    }
    Tracker { feat_names: vec![String::new(); values.len()], feat_values: values.to_vec(), strides, n_regs: acc, columns: vec![], n_cols: 0, trans: vec![] }
}

/// A realised template: string, `bound` (byte start of the last
/// left-reading archiphoneme output: rules may not rewrite before it) and
/// `keep` (length of the protected suffix some right-reading archiphoneme
/// reads: rules may not rewrite inside it).
struct Realised {
    text: String,
    bound: usize,
    keep: usize,
}

impl Phon {
    fn archi(&self, name: &str, line: usize) -> Result<&ArchiM> {
        match self.archis.get(name) {
            Some(a) => Ok(a),
            None => spec_err(line, format!("unknown archiphoneme `{{{name}}}`")),
        }
    }

    /// Does the template contain an archiphoneme that reads right context?
    fn reads_right(&self, tpl: &[Sym], line: usize) -> Result<bool> {
        for s in tpl {
            if let Sym::Archi(n) = s {
                if self.archi(n, line)?.reads != Reads::Left {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// First line whose condition holds. Every ARCHI was checked to be total
    /// over all register pairs, so the empty fallback is never taken.
    fn pick<'a>(&self, a: &'a ArchiM, r: u32, q: u32) -> &'a str {
        a.lines.iter().find(|(c, _)| c.as_ref().is_none_or(|c| cond_ok(&self.left.tracker, &self.right.tracker, c, r, q))).map_or("", |(_, o)| o.as_str())
    }

    /// Realise a template entered with left register `r` and leaving with
    /// right register `q`. Left-readers resolve left to right, right-readers
    /// right to left, a single both-ways reader last — so the result is
    /// unique. Templates must order them L* B? R*.
    fn realize(&self, tpl: &[Sym], r: u32, q: u32, line: usize) -> Result<Realised> {
        let lt = &self.left.tracker;
        let rt = &self.right.tracker;
        // order check + split point
        let mut stage = 0; // 0: L, 1: after B, 2: in R
        let mut split = tpl.len(); // first non-left-reading archi
        for (i, s) in tpl.iter().enumerate() {
            if let Sym::Archi(n) = s {
                let k = self.archi(n, line)?.reads;
                let ok = match k {
                    Reads::Left => stage == 0,
                    Reads::Both => stage == 0,
                    Reads::Right => true,
                };
                if !ok {
                    return spec_err(line, format!("archiphoneme `{{{n}}}` reads left context but follows one that reads right context; order them left-readers, then at most one both-ways reader, then right-readers"));
                }
                if k != Reads::Left && split == tpl.len() {
                    split = i;
                }
                stage = match k {
                    Reads::Left => stage,
                    Reads::Both => 1,
                    Reads::Right => 2,
                };
            }
        }
        let mut outs: Vec<Option<String>> = vec![None; tpl.len()];
        // left pass up to `split`
        let mut cur = r;
        for (i, s) in tpl[..split].iter().enumerate() {
            match s {
                Sym::Ch(c) => cur = lt.step_safe(cur, *c),
                Sym::Archi(n) => {
                    let o = self.pick(self.archi(n, line)?, cur, q).to_string();
                    for c in o.chars() {
                        cur = lt.step_safe(cur, c);
                    }
                    outs[i] = Some(o);
                }
            }
        }
        // right pass down to `split` (exclusive if it is the B reader)
        let b_at = split < tpl.len() && matches!(&tpl[split], Sym::Archi(n) if self.archis[n].reads == Reads::Both);
        let lo = if b_at { split + 1 } else { split };
        let mut rq = q;
        for i in (lo..tpl.len()).rev() {
            match &tpl[i] {
                Sym::Ch(c) => rq = rt.step_safe(rq, *c),
                Sym::Archi(n) => {
                    let o = self.pick(self.archi(n, line)?, r, rq).to_string();
                    for c in o.chars().rev() {
                        rq = rt.step_safe(rq, c);
                    }
                    outs[i] = Some(o);
                }
            }
        }
        if b_at {
            if let Sym::Archi(n) = &tpl[split] {
                outs[split] = Some(self.pick(self.archi(n, line)?, cur, rq).to_string());
            }
        }
        // assemble, tracking spans
        let mut text = String::new();
        let mut bound = 0;
        let mut keep_from: Option<usize> = None; // smallest output end of a right-reader
        for (i, s) in tpl.iter().enumerate() {
            match s {
                Sym::Ch(c) => text.push(*c),
                Sym::Archi(n) => {
                    let start = text.len();
                    text.push_str(outs[i].as_deref().unwrap_or(""));
                    match self.archis[n].reads {
                        Reads::Left => bound = start,
                        Reads::Both => {
                            bound = start;
                            keep_from.get_or_insert(text.len());
                        }
                        Reads::Right => {
                            keep_from.get_or_insert(text.len());
                        }
                    }
                }
            }
        }
        let keep = keep_from.map_or(0, |e| text.len() - e);
        Ok(Realised { text, bound, keep })
    }

    /// Does char `c` match any line of left (or right) feature `f`?
    fn touches(&self, right: bool, f: usize, c: char, cls: &Classes) -> bool {
        let d = if right { &self.right } else { &self.left };
        d.feats[f].lines.iter().any(|(_, atoms, _)| atoms.iter().any(|a| atom_matches(a, cls, c)))
    }
}

impl Tracker {
    fn step_safe(&self, r: u32, c: char) -> u32 {
        if self.n_regs == 1 {
            0
        } else {
            self.step(r, c)
        }
    }
    /// Run a right-context register backward over `s` (last char first).
    pub fn run_back(&self, mut q: u32, s: &str) -> u32 {
        if self.n_regs == 1 {
            return 0;
        }
        for c in s.chars().rev() {
            q = self.step(q, c);
        }
        q
    }
}

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

enum TargetM {
    Any,
    Class(ClassRef),
    Lit(String),
}

enum LeftM {
    Any,
    Boundary,
    Class(ClassRef),
    Lit(String),
}

struct RuleM {
    name: String,
    optional: bool,
    target: TargetM,
    repl: String,
    left: LeftM,
    tail: Vec<(AtomM, Quant)>,
    right_ids: BTreeSet<u32>,
    /// `+[CLASS]` contexts are narrowed to the morphemes that can follow.
    class_based: bool,
    flag: Option<String>,
    line: usize,
}

fn tail_match(atoms: &[(AtomM, Quant)], cs: &[char], cls: &Classes) -> bool {
    let Some(((a, q), rest)) = atoms.split_first() else { return cs.is_empty() };
    let m = |c: char| atom_matches(a, cls, c);
    match q {
        Quant::One => !cs.is_empty() && m(cs[0]) && tail_match(rest, &cs[1..], cls),
        Quant::Opt => tail_match(rest, cs, cls) || (!cs.is_empty() && m(cs[0]) && tail_match(rest, &cs[1..], cls)),
        Quant::Star | Quant::Plus => {
            let min = if *q == Quant::Plus { 1 } else { 0 };
            let mut k = 0;
            while k < cs.len() && m(cs[k]) {
                k += 1;
            }
            (min..=k).rev().any(|j| tail_match(rest, &cs[j..], cls))
        }
    }
}

impl RuleM {
    /// Rightmost match: (start byte, rewritten string).
    /// Rightmost match: (start byte, end byte of the target, rewritten string).
    fn apply(&self, s: &str, cls: &Classes) -> Option<(usize, usize, String)> {
        if self.tail.is_empty() {
            // Common case: the target must end the string — one candidate.
            let st = match &self.target {
                TargetM::Lit(t) => s.strip_suffix(t.as_str()).map(|p| p.len())?,
                TargetM::Any => s.len() - s.chars().next_back()?.len_utf8(),
                TargetM::Class(r) => {
                    let c = s.chars().next_back()?;
                    if !cls.has_char(*r, c) {
                        return None;
                    }
                    s.len() - c.len_utf8()
                }
            };
            return self.finish_match(s, st, s.len() - st, cls);
        }
        for (st, c) in s.char_indices().rev() {
            let tlen = match &self.target {
                TargetM::Lit(t) => {
                    if !s[st..].starts_with(t.as_str()) {
                        continue;
                    }
                    t.len()
                }
                TargetM::Any => c.len_utf8(),
                TargetM::Class(r) => {
                    if !cls.has_char(*r, c) {
                        continue;
                    }
                    c.len_utf8()
                }
            };
            let after: Vec<char> = s[st + tlen..].chars().collect();
            if !tail_match(&self.tail, &after, cls) {
                continue;
            }
            if let Some(m) = self.finish_match(s, st, tlen, cls) {
                return Some(m);
            }
        }
        None
    }

    fn finish_match(&self, s: &str, st: usize, tlen: usize, cls: &Classes) -> Option<(usize, usize, String)> {
        let pre = &s[..st];
        let ok = match &self.left {
            LeftM::Any => true,
            LeftM::Boundary => pre.is_empty(),
            LeftM::Class(r) => pre.chars().next_back().is_some_and(|c| cls.has_char(*r, c)),
            LeftM::Lit(l) => pre.ends_with(l.as_str()),
        };
        if !ok {
            return None;
        }
        let matched = &s[st..st + tlen];
        Some((st, st + tlen, format!("{pre}{}{}", self.repl.replace('$', matched), &s[st + tlen..])))
    }
}

// ---------------------------------------------------------------------------
// Grammar compilation
// ---------------------------------------------------------------------------

struct Base<'a> {
    lexicon: ClassId,
    e: &'a SpecEntry,
    cont: ClassId,
    tpl: Vec<Sym>,
}

/// A broken compiler invariant, reported as an error instead of a panic.
fn internal<T>(line: usize, what: &str) -> Result<T> {
    spec_err(line, format!("internal compiler error: {what}"))
}

pub fn compile_grammar(spec: &Spec) -> Result<Grammar> {
    // ---- classes, phonology ---------------------------------------------
    let cls = Classes::build(spec)?;
    let phon = build_phon(spec, &cls)?;

    // ---- lexicons -------------------------------------------------------
    let mut classes = vec![ClassInfo { name: "#".into(), open_tag: None }];
    let mut ids: BTreeMap<&str, ClassId> = BTreeMap::new();
    ids.insert("#", END);
    for lex in &spec.lexicons {
        if ids.contains_key(lex.name.as_str()) {
            return spec_err(lex.line, format!("duplicate LEXICON `{}`", lex.name));
        }
        if classes.len() >= u16::MAX as usize {
            return spec_err(lex.line, "too many lexicons");
        }
        ids.insert(&lex.name, classes.len() as ClassId);
        classes.push(ClassInfo { name: lex.name.clone(), open_tag: lex.open_tag.clone() });
    }
    let Some(&root) = ids.get("Root") else {
        return spec_err(0, "missing `LEXICON Root`");
    };
    let mut bases: Vec<Base> = Vec::new();
    let mut epsilons: Vec<(ClassId, ClassId)> = Vec::new();
    for lex in &spec.lexicons {
        let lid = ids[lex.name.as_str()];
        for e in &lex.entries {
            let Some(&cont) = ids.get(e.cont.as_str()) else {
                return spec_err(e.line, format!("unknown continuation class `{}`", e.cont));
            };
            if e.is_epsilon() {
                if !epsilons.contains(&(lid, cont)) {
                    epsilons.push((lid, cont));
                }
            } else {
                if e.surface.chars().any(char::is_whitespace) {
                    return spec_err(e.line, "whitespace in surface");
                }
                let tpl = parse_template(&e.surface).map_err(|m| crate::Error::Spec { line: e.line, msg: m })?;
                bases.push(Base { lexicon: lid, e, cont, tpl });
            }
        }
    }
    check_epsilon_cycles(&classes, &epsilons)?;

    // ---- lexical ids (sorted templates => deterministic) -----------------
    let lex_set: BTreeSet<&str> = bases.iter().map(|b| b.e.surface.as_str()).collect();
    let lex_surfaces: Vec<String> = lex_set.iter().map(|s| s.to_string()).collect();
    let lex_id = |s: &str| lex_surfaces.binary_search_by(|x| x.as_str().cmp(s)).ok().map(|i| i as u32);
    let first_sym: Vec<Option<Sym>> = lex_surfaces.iter().map(|s| parse_template(s).ok().and_then(|t| t.into_iter().next())).collect();

    // next_lex[c] = lexical ids that can be the next morpheme after entering c
    let mut ids_by_class: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); classes.len()];
    for b in &bases {
        let Some(id) = lex_id(&b.e.surface) else { return internal(b.e.line, "template not interned") };
        ids_by_class[b.lexicon as usize].insert(id);
    }
    let next_lex: Vec<BTreeSet<u32>> = (0..classes.len())
        .map(|c| closure(&epsilons, c as ClassId).into_iter().flat_map(|k| ids_by_class[k as usize].iter().copied()).collect())
        .collect();

    // ---- rules ------------------------------------------------------------
    let mut rules: Vec<RuleM> = Vec::new();
    for r in &spec.rules {
        let target = match &r.target {
            Target::AnyChar => TargetM::Any,
            Target::Class(n) => TargetM::Class(cls.resolve(n, r.line)?),
            Target::Literal(t) => TargetM::Lit(t.clone()),
        };
        let left = match &r.left {
            LeftCtx::Any => LeftM::Any,
            LeftCtx::Boundary => LeftM::Boundary,
            LeftCtx::Consonant => LeftM::Class(cls.resolve("C", r.line)?),
            LeftCtx::Vowel => LeftM::Class(cls.resolve("V", r.line)?),
            LeftCtx::Class(n) => LeftM::Class(cls.resolve(n, r.line)?),
            LeftCtx::Literal(l) => LeftM::Lit(l.clone()),
        };
        let tail = r.tail.iter().map(|(a, q)| Ok((compile_atom(a, &cls, r.line)?, *q))).collect::<Result<Vec<_>>>()?;
        let right_ids: BTreeSet<u32> = match &r.right {
            RightCtx::Lex(v) => v.iter().filter_map(|s| lex_id(s)).collect(),
            RightCtx::Class(n) => {
                let cr = cls.resolve(n, r.line)?;
                (0..lex_surfaces.len() as u32).filter(|&i| first_sym[i as usize].as_ref().is_some_and(|s| cls.has_sym(cr, s))).collect()
            }
        };
        rules.push(RuleM {
            name: r.name.clone(),
            optional: r.optional,
            target,
            repl: r.replacement.clone(),
            left,
            tail,
            right_ids,
            class_based: matches!(r.right, RightCtx::Class(_)),
            flag: r.require_flag.clone(),
            line: r.line,
        });
    }

    // reaches[r][c]: can rule r's right context follow a morpheme whose
    // continuation is class c?
    let reaches: Vec<Vec<bool>> =
        rules.iter().map(|r| next_lex.iter().map(|nl| r.right_ids.iter().any(|id| nl.contains(id))).collect()).collect();

    // ---- pinnable features (@set) -----------------------------------------
    let mut pinnable: Vec<u16> = Vec::new();
    let mut rpinnable: Vec<u16> = Vec::new();
    for b in &bases {
        for f in &b.e.flags {
            let (kv, right) = match (f.strip_prefix("set:"), f.strip_prefix("rset:")) {
                (Some(kv), _) => (kv, false),
                (_, Some(kv)) => (kv, true),
                _ => continue,
            };
            let Some((feat, _)) = kv.split_once('=') else { return spec_err(b.e.line, "expected @set:feature=value / @rset:feature=value") };
            let (own, other, list) = if right { (&phon.right, &phon.left, &mut rpinnable) } else { (&phon.left, &phon.right, &mut pinnable) };
            let Some(&fi) = own.index.get(feat) else {
                if other.index.contains_key(feat) {
                    let (k, want) = if right { ("@rset", "RFEATURE") } else { ("@set", "FEATURE") };
                    return spec_err(b.e.line, format!("{k} needs a {want}; `{feat}` is the other direction (use @set for FEATUREs, @rset for RFEATUREs)"));
                }
                return spec_err(b.e.line, format!("unknown FEATURE `{feat}`"));
            };
            if !list.contains(&(fi as u16)) {
                list.push(fi as u16);
            }
        }
    }
    // INDUCE features are pinnable too (learned / guessed exceptions).
    let mut induce_classes: Vec<ClassId> = Vec::new();
    let mut induce_feats: Vec<(bool, u16)> = Vec::new();
    for d in &spec.induce {
        for f in &d.features {
            let (right, fi) = match (phon.left.index.get(f), phon.right.index.get(f)) {
                (Some(&i), _) => (false, i as u16),
                (_, Some(&i)) => (true, i as u16),
                _ => return spec_err(d.line, format!("INDUCE: unknown FEATURE `{f}`")),
            };
            let list = if right { &mut rpinnable } else { &mut pinnable };
            if !list.contains(&fi) {
                list.push(fi);
            }
            if !induce_feats.contains(&(right, fi)) {
                induce_feats.push((right, fi));
            }
        }
        for l in &d.lexicons {
            match ids.get(l.as_str()) {
                Some(&c) if c != END => {
                    if !induce_classes.contains(&c) {
                        induce_classes.push(c);
                    }
                }
                _ => return spec_err(d.line, format!("INDUCE: unknown LEXICON `{l}`")),
            }
        }
    }
    induce_classes.sort_unstable();
    pinnable.sort_unstable();
    rpinnable.sort_unstable();
    // Every induced feature was pushed onto `pinnable` / `rpinnable` above.
    let pin_index = |list: &[u16], f: u16| list.iter().position(|&p| p == f).map(|i| i as u16);
    let induce_left: Vec<u16> = induce_feats.iter().filter(|(r, _)| !r).filter_map(|&(_, f)| pin_index(&pinnable, f)).collect();
    let induce_right: Vec<u16> = induce_feats.iter().filter(|(r, _)| *r).filter_map(|&(_, f)| pin_index(&rpinnable, f)).collect();
    let mut pin_space: u32 = 1;
    for &f in &pinnable {
        pin_space *= phon.left.tracker.feat_values[f as usize].len() as u32 + 1;
        if pin_space > MAX_PINS {
            return spec_err(0, "too many pinned feature combinations");
        }
    }
    let mut rpin_space: u32 = 1;
    for &f in &rpinnable {
        rpin_space *= phon.right.tracker.feat_values[f as usize].len() as u32 + 2;
        if rpin_space > MAX_PINS {
            return spec_err(0, "too many right-pinned feature combinations");
        }
    }

    let mut g = Grammar {
        classes,
        root,
        entries: Vec::new(),
        epsilons,
        filters: vec![Filter::none()],
        lex_surfaces: Vec::new(),
        extra_pieces: Vec::new(),
        options: spec.options.clone(),
        tracker: phon.left.tracker.clone(),
        rtracker: phon.right.tracker.clone(),
        guards: vec![Vec::new()],
        pinnable: pinnable.clone(),
        rpinnable: rpinnable.clone(),
        induce_left,
        induce_right,
        induce_classes,
    };
    let mut filter_ids: BTreeMap<Filter, u16> = BTreeMap::new();
    filter_ids.insert(Filter::none(), 0);
    let mut intern = |g: &mut Grammar, f: Filter| -> Result<u16> {
        if let Some(&i) = filter_ids.get(&f) {
            return Ok(i);
        }
        if g.filters.len() >= u16::MAX as usize {
            return spec_err(0, "too many distinct rule filters");
        }
        let i = g.filters.len() as u16;
        g.filters.push(f.clone());
        filter_ids.insert(f, i);
        Ok(i)
    };
    let mut guard_ids: BTreeMap<Vec<u64>, u32> = BTreeMap::new();
    let (n_l, n_r) = (phon.left.tracker.n_regs, phon.right.tracker.n_regs);
    let n_pairs = n_l * n_r;
    let words = (n_pairs as usize).div_ceil(64);

    let mut considered: Vec<&RuleM> = Vec::new();
    let mut matching: Vec<bool> = Vec::new();
    let underlying = spec.options.underlying;
    for b in &bases {
        let Some(my_id) = lex_id(&b.e.surface) else { return internal(b.e.line, "template not interned") };
        // flags
        let mut pin_set = Vec::new();
        let mut rpin_set = Vec::new();
        let mut allow_next: Option<BTreeSet<u32>> = None;
        let mut tier_override: Option<String> = None;
        for f in &b.e.flags {
            if let Some(t) = f.strip_prefix("tier=") {
                if !spec.options.underlying {
                    return spec_err(b.e.line, "@tier= needs `OPTION harmony underlying`");
                }
                tier_override = Some(t.to_string());
                continue;
            }
            let pinned = match (f.strip_prefix("set:"), f.strip_prefix("rset:")) {
                (Some(kv), _) => Some((kv, false)),
                (_, Some(kv)) => Some((kv, true)),
                _ => None,
            };
            if let Some((kv, right)) = pinned {
                let Some((feat, val)) = kv.split_once('=') else {
                    return spec_err(b.e.line, "expected @set:feature=value / @rset:feature=value");
                };
                let (d, list) = if right { (&phon.right, &rpinnable) } else { (&phon.left, &pinnable) };
                let fi = d.index[feat];
                let Some(vi) = d.tracker.feat_values[fi].iter().position(|v| v == val) else {
                    return spec_err(b.e.line, format!("feature `{feat}` has no value `{val}`"));
                };
                let Some(pi) = list.iter().position(|&p| p as usize == fi) else { return internal(b.e.line, "pinned feature not registered") };
                let pi = pi as u16;
                if right { rpin_set.push((pi, vi as u16)) } else { pin_set.push((pi, vi as u16)) }
            } else if let Some(cn) = f.strip_prefix("next=") {
                let cr = cls.resolve(cn, b.e.line)?;
                allow_next = Some((0..lex_surfaces.len() as u32).filter(|&i| first_sym[i as usize].as_ref().is_some_and(|s| cls.has_sym(cr, s))).collect());
            }
        }

        // 1. archiphoneme variants, grouped by realised string. A variant
        //    records the (left, right) register pairs `r·R_r + q` producing it.
        let has_archi = b.tpl.iter().any(|s| matches!(s, Sym::Archi(_)));
        let reads_right = has_archi && phon.reads_right(&b.tpl, b.e.line)?;
        // (text, pairs, bound, keep)
        let mut variants: Vec<(String, Vec<u32>, usize, usize)> = Vec::new();
        if !has_archi {
            variants.push((b.e.surface.clone(), Vec::new(), 0, 0));
        } else {
            let mut by: BTreeMap<String, usize> = BTreeMap::new();
            // Left-only templates do not depend on q: realise once per r.
            let qs = if reads_right { n_r } else { 1 };
            for r in 0..n_l {
                for q in 0..qs {
                    let rz = phon.realize(&b.tpl, r, q, b.e.line)?;
                    let pairs: Vec<u32> = if reads_right { vec![r * n_r + q] } else { (0..n_r).map(|q| r * n_r + q).collect() };
                    match by.get(&rz.text) {
                        Some(&i) => {
                            variants[i].1.extend(pairs);
                            variants[i].2 = variants[i].2.max(rz.bound);
                            variants[i].3 = variants[i].3.max(rz.keep);
                        }
                        None => {
                            by.insert(rz.text.clone(), variants.len());
                            variants.push((rz.text, pairs, rz.bound, rz.keep));
                        }
                    }
                }
            }
            if variants.len() == 1 {
                variants[0].1.clear(); // realised identically everywhere
            }
        }

        for (vs, regs, bound, keep) in variants {
            if vs.is_empty() {
                return spec_err(b.e.line, format!("`{}` realises as the empty string", b.e.surface));
            }
            let guard = if regs.is_empty() || regs.len() as u32 == n_pairs {
                0
            } else {
                let mut bits = vec![0u64; words];
                for r in &regs {
                    bits[*r as usize / 64] |= 1 << (r % 64);
                }
                let next = guard_ids.len() as u32 + 1;
                let id = *guard_ids.entry(bits.clone()).or_insert(next);
                if id == next {
                    g.guards.push(bits);
                }
                id
            };

            // 2. rules
            considered.clear();
            considered.extend(
                rules
                    .iter()
                    .enumerate()
                    .filter(|(ri, r)| reaches[*ri][b.cont as usize] && r.flag.as_ref().is_none_or(|f| b.e.flags.contains(f)))
                    .map(|(_, r)| r),
            );
            // Only next morphemes reachable through a rule that matches the
            // original string can change anything: if none of an id's rules
            // matches, the first cannot fire, so none fires (no feeding).
            matching.clear();
            matching.extend(considered.iter().map(|r| r.apply(&vs, &cls).is_some()));
            let any_match = matching.iter().any(|&m| m);
            let mut deny: BTreeSet<u32> = BTreeSet::new();
            // result string -> (allowed next ids, rule sequence of first derivation)
            let mut others: BTreeMap<String, (BTreeSet<u32>, Vec<usize>)> = BTreeMap::new();
            if any_match {
                let nl = &next_lex[b.cont as usize];
                let cands: BTreeSet<u32> = considered
                    .iter()
                    .zip(&matching)
                    .filter(|(_, &m)| m)
                    .flat_map(|(r, _)| r.right_ids.iter().copied().filter(move |id| !r.class_based || nl.contains(id)))
                    .collect();
                // Ids to which the same considered rules apply give the same
                // results: evaluate once per signature.
                // Signature: bitset of applicable considered rules.
                let words = considered.len().div_ceil(64).max(1);
                let mut groups: BTreeMap<Vec<u64>, Vec<u32>> = BTreeMap::new();
                let mut sig = vec![0u64; words];
                for id in cands {
                    sig.iter_mut().for_each(|w| *w = 0);
                    for (ri, r) in considered.iter().enumerate() {
                        if r.right_ids.contains(&id) {
                            sig[ri / 64] |= 1 << (ri % 64);
                        }
                    }
                    match groups.get_mut(&sig) {
                        Some(v) => v.push(id),
                        None => {
                            groups.insert(sig.clone(), vec![id]);
                        }
                    }
                }
                for (sig, ids) in groups {
                    let mut states: Vec<(String, Vec<usize>)> = vec![(vs.clone(), vec![])];
                    for (ri, r) in considered.iter().enumerate() {
                        if sig[ri / 64] >> (ri % 64) & 1 == 0 {
                            continue;
                        }
                        let mut nxt: Vec<(String, Vec<usize>)> = Vec::new();
                        for (cur, seq) in &states {
                            match r.apply(cur, &cls) {
                                Some((start, end, new)) => {
                                    // (In underlying mode registers never read rule
                                    // output, so rules may rewrite anything.)
                                    if !underlying && start < bound {
                                        return spec_err(r.line, format!("rule `{}` rewrites `{}` before an archiphoneme's realisation", r.name, cur));
                                    }
                                    if !underlying && end > cur.len() - keep {
                                        return spec_err(r.line, format!("rule `{}` rewrites `{}` inside text an archiphoneme reads as right context", r.name, cur));
                                    }
                                    let mut s2 = seq.clone();
                                    s2.push(ri);
                                    nxt.push((new, s2));
                                    if r.optional {
                                        nxt.push((cur.clone(), seq.clone()));
                                    }
                                }
                                None => nxt.push((cur.clone(), seq.clone())),
                            }
                        }
                        let mut seen = BTreeSet::new();
                        nxt.retain(|(s, _)| seen.insert(s.clone()));
                        states = nxt;
                    }
                    if !states.iter().any(|(s, _)| *s == vs) {
                        deny.extend(ids.iter().copied());
                    }
                    for (s, seq) in states {
                        if s == vs {
                            continue;
                        }
                        if s.is_empty() {
                            let r = considered[seq[0]];
                            return spec_err(r.line, format!("rule `{}` deletes all of `{}`", r.name, vs));
                        }
                        let slot = others.entry(s).or_insert_with(|| (BTreeSet::new(), seq.clone()));
                        slot.0.extend(ids.iter().copied());
                        if seq < slot.1 {
                            slot.1 = seq;
                        }
                    }
                }
            }

            // 3. filters (+ @next) and entries
            let restrict = |ids: BTreeSet<u32>| -> BTreeSet<u32> {
                match &allow_next {
                    Some(a) => ids.intersection(a).copied().collect(),
                    None => ids,
                }
            };
            let tier: Option<String> = underlying.then(|| tier_override.clone().unwrap_or_else(|| vs.clone()));
            let mk = |g: &mut Grammar, surface: String, filter: u16, rule: Option<String>| {
                let reg_text = tier.clone().unwrap_or_else(|| surface.clone());
                let touch = |list: &[u16], right: bool| {
                    if list.is_empty() {
                        return 0;
                    }
                    list.iter().enumerate().fold(0u32, |m, (i, &f)| {
                        if reg_text.chars().any(|c| phon.touches(right, f as usize, c, &cls)) {
                            m | 1 << i
                        } else {
                            m
                        }
                    })
                };
                let pt = touch(&pinnable, false);
                let rpt = touch(&rpinnable, true);
                g.entries.push(Entry {
                    lexicon: b.lexicon,
                    surface,
                    lex_surface: b.e.surface.clone(),
                    lex_id: my_id,
                    lemma: b.e.lemma.clone(),
                    tag: b.e.tag.clone(),
                    cont: b.cont,
                    filter,
                    rule,
                    guard,
                    pin_set: pin_set.clone(),
                    pin_touch: pt,
                    rpin_set: rpin_set.clone(),
                    rpin_touch: rpt,
                    reads_right,
                    tier: tier.clone(),
                });
            };
            let base_filter = match &allow_next {
                Some(a) => {
                    let ids: BTreeSet<u32> = a.difference(&deny).copied().collect();
                    if ids.is_empty() {
                        None
                    } else {
                        Some(intern(&mut g, Filter { kind: FilterKind::Allow, ids: ids.into_iter().collect() })?)
                    }
                }
                None if deny.is_empty() => Some(0),
                None => Some(intern(&mut g, Filter { kind: FilterKind::Deny, ids: deny.iter().copied().collect() })?),
            };
            if let Some(f) = base_filter {
                mk(&mut g, vs.clone(), f, None);
            }
            let mut ordered: Vec<(String, BTreeSet<u32>, Vec<usize>)> = others.into_iter().map(|(s, (ids, seq))| (s, ids, seq)).collect();
            ordered.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.cmp(&b.0)));
            for (s, ids, seq) in ordered {
                let ids = restrict(ids);
                if ids.is_empty() {
                    continue;
                }
                let f = intern(&mut g, Filter { kind: FilterKind::Allow, ids: ids.into_iter().collect() })?;
                let name = seq.iter().map(|&i| considered[i].name.as_str()).collect::<Vec<_>>().join("+");
                mk(&mut g, s, f, Some(name));
            }
        }
    }
    if g.entries.len() >= u32::MAX as usize / 2 {
        return spec_err(0, "too many entries");
    }

    // ---- prune guards to reachable registers (only with a real register) --
    if n_l > 1 || n_r > 1 {
        prune_unreachable(&mut g);
    }

    // ---- extra pieces ---------------------------------------------------
    for (p, line) in &spec.pieces {
        let (text, cont) = match p.strip_prefix("##") {
            Some(rest) if !rest.is_empty() => (rest, true),
            _ => (p.as_str(), false),
        };
        if text.is_empty() {
            return spec_err(*line, "empty piece");
        }
        g.extra_pieces.push((text.to_string(), cont));
    }
    g.lex_surfaces = lex_surfaces;
    Ok(g)
}

/// Registers reachable from `none` over any characters.
fn globally_reachable(t: &Tracker) -> Vec<bool> {
    let mut seen = vec![false; t.n_regs as usize];
    seen[0] = true;
    let mut stack = vec![0u32];
    while let Some(r) = stack.pop() {
        for col in 0..t.n_cols as u32 {
            let r2 = t.trans[(r * t.n_cols as u32 + col) as usize] as usize;
            if !seen[r2] {
                seen[r2] = true;
                stack.push(r2 as u32);
            }
        }
    }
    seen
}

/// Fixpoints over (class, register) reachability in both directions:
/// left registers flow forward from `Root`, right registers flow backward
/// from `#`. Entries whose guard admits no reachable (left, right) pair are
/// dropped (they would only add junk pieces and trie keys). Open lexicons
/// may neighbour an unknown stem in hybrid mode, so they admit every
/// register the trackers can produce; right pins make all right values
/// reachable.
fn prune_unreachable(g: &mut Grammar) {
    let t = &g.tracker;
    let rt = &g.rtracker;
    let (n, nr) = (t.n_regs as usize, rt.n_regs as usize);
    let nc = g.classes.len();
    let closures: Vec<Vec<ClassId>> = (0..nc).map(|c| g.class_closure(c as ClassId)).collect();
    let open: Vec<bool> = (0..nc).map(|c| closures[c].iter().any(|&k| g.classes[k as usize].open_tag.is_some())).collect();

    // ---- right registers at the point where each class is entered --------
    let mut rq: Vec<Vec<bool>> = vec![vec![false; nr]; nc];
    if nr == 1 || !g.rpinnable.is_empty() {
        for row in &mut rq {
            row.iter_mut().for_each(|x| *x = true);
        }
    } else {
        let global_r = globally_reachable(rt);
        for (c, row) in rq.iter_mut().enumerate() {
            if closures[c].contains(&END) {
                row[0] = true;
            }
            if open[c] {
                *row = global_r.clone();
            }
        }
        // classes whose closure contains lexicon L
        let mut containing: Vec<Vec<usize>> = vec![Vec::new(); nc];
        for (c, clo) in closures.iter().enumerate() {
            for &k in clo {
                containing[k as usize].push(c);
            }
        }
        loop {
            let mut changed = false;
            for e in &g.entries {
                for q in 0..nr {
                    if !rq[e.cont as usize][q] || !(0..n as u32).any(|r| g.guard_has(e.guard, r, q as u32)) {
                        continue;
                    }
                    let q0 = rt.run_back(q as u32, e.register_text()) as usize;
                    for &c in &containing[e.lexicon as usize] {
                        if !rq[c][q0] {
                            rq[c][q0] = true;
                            changed = true;
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }

    // ---- left registers ---------------------------------------------------
    let global = globally_reachable(t);
    let pin_apply = |r: u32, e: &Entry| -> u32 {
        let mut r = r;
        for &(pi, v) in &e.pin_set {
            r = t.with(r, g.pinnable[pi as usize] as usize, v as usize);
        }
        r
    };
    let admits = |e: &Entry, r: u32| (0..nr as u32).any(|q| rq[e.cont as usize][q as usize] && g.guard_has(e.guard, r, q));
    let mut reach: Vec<Vec<bool>> = vec![vec![false; n]; nc];
    for &c in &closures[g.root as usize] {
        reach[c as usize][0] = true;
    }
    for c in 0..nc {
        if g.classes[c].open_tag.is_some() {
            for e in g.entries.iter().filter(|e| e.lexicon as usize == c) {
                for &c2 in &closures[e.cont as usize] {
                    for (r, ok) in global.iter().enumerate() {
                        if *ok {
                            reach[c2 as usize][r] = true;
                        }
                    }
                }
            }
        }
    }
    loop {
        let mut changed = false;
        for e in &g.entries {
            let src: Vec<u32> = (0..n as u32).filter(|&r| reach[e.lexicon as usize][r as usize] && admits(e, r)).collect();
            if src.is_empty() {
                continue;
            }
            let conts = &closures[e.cont as usize];
            for r in src {
                let r2 = pin_apply(t.run(r, e.register_text()), e) as usize;
                for &c2 in conts {
                    if !reach[c2 as usize][r2] {
                        reach[c2 as usize][r2] = true;
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    let words = (n * nr).div_ceil(64);
    // Decide everything with shared borrows, then rebuild.
    let plan: Vec<Option<Option<Vec<u64>>>> = g
        .entries
        .iter()
        .map(|e| {
            if !(0..n as u32).any(|r| reach[e.lexicon as usize][r as usize] && admits(e, r)) {
                return None; // unusable: drop
            }
            // Guards keep their full register set; re-intern them densely.
            Some((e.guard != 0).then(|| {
                let mut bits = vec![0u64; words];
                for i in 0..n * nr {
                    if g.guard_has(e.guard, (i / nr) as u32, (i % nr) as u32) {
                        bits[i / 64] |= 1 << (i % 64);
                    }
                }
                bits
            }))
        })
        .collect();
    let mut new_guards: Vec<Vec<u64>> = vec![Vec::new()];
    let mut ids: BTreeMap<Vec<u64>, u32> = BTreeMap::new();
    let mut kept = Vec::with_capacity(g.entries.len());
    for (mut e, p) in std::mem::take(&mut g.entries).into_iter().zip(plan) {
        let Some(bits) = p else { continue };
        if let Some(bits) = bits {
            let next = new_guards.len() as u32;
            let id = *ids.entry(bits.clone()).or_insert(next);
            if id == next {
                new_guards.push(bits);
            }
            e.guard = id;
        }
        kept.push(e);
    }
    g.entries = kept;
    g.guards = new_guards;
}

fn check_epsilon_cycles(classes: &[ClassInfo], eps: &[(ClassId, ClassId)]) -> Result<()> {
    let n = classes.len();
    let mut adj = vec![Vec::new(); n];
    for &(a, b) in eps {
        adj[a as usize].push(b as usize);
    }
    let mut color = vec![0u8; n];
    for s in 0..n {
        if color[s] != 0 {
            continue;
        }
        let mut stack = vec![(s, 0usize)];
        color[s] = 1;
        while let Some(&mut (v, ref mut i)) = stack.last_mut() {
            if *i < adj[v].len() {
                let w = adj[v][*i];
                *i += 1;
                match color[w] {
                    0 => {
                        color[w] = 1;
                        stack.push((w, 0));
                    }
                    1 => {
                        return spec_err(
                            0,
                            format!("epsilon cycle through `{}` and `{}`", classes[v].name, classes[w].name),
                        )
                    }
                    _ => {}
                }
            } else {
                color[v] = 2;
                stack.pop();
            }
        }
    }
    Ok(())
}
