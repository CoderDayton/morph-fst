//! An *independent* string-level reference implementation of the spec
//! semantics, used as a test oracle and as a benchmark corpus source.
//!
//! It works on the raw AST with its own interpreter — no compiled tracker
//! tables, no allomorph expansion, no filters, no guards, no automata, and
//! not the compiler's ordered archiphoneme resolution:
//!
//! 1. enumerate morpheme paths from `Root` to `#`;
//! 2. realise them left to right: archiphonemes that read only left
//!    context are resolved from the left register (interpreted FEATURE
//!    lines over the realised text so far, with `@set` pins); archiphonemes
//!    that read right context *branch* over all their outputs; rules
//!    rewrite each morpheme given the next one's lexical template;
//! 3. once the whole surface is known, compute the right register
//!    (RFEATURE lines run backward from the word's end, with `@rset` pins)
//!    and keep a realisation only if every branched archiphoneme's choice is
//!    the first line whose condition holds (branch-and-verify);
//! 4. emit negatives: rule-less realisations, single wrong left
//!    archiphonemes, and every rejected branch.
//!
//! If the compiled analyzers agree with this, the compilation is faithful.
//!
//! [`generate`] first checks that the spec compiles, so every name lookup
//! below succeeds; a failed lookup falls back to "no match" rather than
//! panicking.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::error::Result;
use crate::spec::{is_consonant, is_vowel, parse_template, Atom, LeftCtx, Quant, RightCtx, Spec, SpecEntry, SpecRule, Sym, Target};

pub type Gloss = Vec<(String, String)>;

#[derive(Debug, Default)]
pub struct Generated {
    /// surface → set of (lemma, tag) sequences.
    pub surfaces: BTreeMap<String, BTreeSet<Gloss>>,
    /// Realisations with no rules applied (candidate negatives).
    pub naive: BTreeSet<String>,
    /// Realisations with exactly one archiphoneme realised wrongly, plus
    /// rejected right-context branches.
    pub wrong_archi: BTreeSet<String>,
}

struct Interp<'a> {
    spec: &'a Spec,
    classes: HashMap<&'a str, (BTreeSet<char>, BTreeSet<&'a str>)>,
    /// value names per feature (own numbering), all directions
    values: Vec<Vec<&'a str>>,
    /// left (`@set`) and right (`@rset`) pinnable feature indices
    lpins: Vec<usize>,
    rpins: Vec<usize>,
}

/// Register over *all* features; left steps update FEATUREs only, right
/// steps RFEATUREs only.
type Reg = Vec<usize>;
type Pins = Vec<Option<usize>>;

/// A branched (right-reading) archiphoneme awaiting verification.
#[derive(Clone)]
struct Check {
    morph: usize,
    archi: String,
    chosen: String,
    left: Reg,
    /// base text of the morpheme after the archiphoneme
    suffix: String,
}

/// DFS frame: (morpheme index, left register, left pins, pieces, register
/// texts, checks). Register texts = pieces in surface mode, underlying
/// forms (pre-rule, or `@tier=`) in underlying mode.
type Frame = (usize, Reg, Pins, Vec<String>, Vec<String>, Vec<Check>);
/// Partial realisation: (text, left register, checks with suffix offsets).
type Partial = (String, Reg, Vec<(Check, usize)>);

impl<'a> Interp<'a> {
    fn new(spec: &'a Spec) -> Self {
        let mut classes = HashMap::new();
        for c in &spec.classes {
            let mut ch = BTreeSet::new();
            let mut ar = BTreeSet::new();
            for m in &c.members {
                match m {
                    Sym::Ch(x) => {
                        ch.insert(*x);
                    }
                    Sym::Archi(a) => {
                        ar.insert(a.as_str());
                    }
                }
            }
            classes.insert(c.name.as_str(), (ch, ar));
        }
        let values = spec
            .features
            .iter()
            .map(|f| {
                let mut v = vec!["none"];
                for l in &f.lines {
                    if !v.contains(&l.value.as_str()) {
                        v.push(&l.value);
                    }
                }
                v
            })
            .collect();
        let (mut lpins, mut rpins) = (Vec::new(), Vec::new());
        for l in &spec.lexicons {
            for e in &l.entries {
                for f in &e.flags {
                    for (pre, list) in [("set:", &mut lpins), ("rset:", &mut rpins)] {
                        if let Some((feat, _)) = f.strip_prefix(pre).and_then(|kv| kv.split_once('=')) {
                            let Some(i) = spec.features.iter().position(|x| x.name == feat) else { continue };
                            if !list.contains(&i) {
                                list.push(i);
                            }
                        }
                    }
                }
            }
        }
        Interp { spec, classes, values, lpins, rpins }
    }

    fn zero(&self) -> Reg {
        vec![0; self.spec.features.len()]
    }

    fn in_class(&self, name: &str, c: char) -> bool {
        match self.classes.get(name) {
            Some((ch, _)) => ch.contains(&c),
            None if name == "C" => is_consonant(c),
            None if name == "V" => is_vowel(c),
            None => false,
        }
    }

    fn sym_in_class(&self, name: &str, s: &Sym) -> bool {
        match s {
            Sym::Ch(c) => self.in_class(name, *c),
            Sym::Archi(a) => self.classes.get(name).is_some_and(|(_, ar)| ar.contains(a.as_str())),
        }
    }

    fn atom_hit(&self, a: &Atom, c: char) -> bool {
        match a {
            Atom::Ch(x) => *x == c,
            Atom::Class(n) => self.in_class(n, c),
            Atom::Any => true,
        }
    }

    fn feat(&self, name: &str) -> Option<usize> {
        self.spec.features.iter().position(|x| x.name == name)
    }

    /// Condition over a left register `l` and a right register `r`.
    fn cond_holds(&self, cond: &[(String, Vec<String>)], l: &Reg, r: &Reg) -> bool {
        cond.iter().all(|(f, vs)| {
            let Some(fi) = self.feat(f) else { return false };
            let v = if self.spec.features[fi].right { r[fi] } else { l[fi] };
            vs.iter().any(|x| self.values[fi][v] == x)
        })
    }

    /// One step of the left (`right == false`) or right register.
    fn step(&self, reg: &Reg, c: char, right: bool) -> Reg {
        let mut out = reg.clone();
        for (fi, f) in self.spec.features.iter().enumerate() {
            if f.right != right {
                continue;
            }
            if let Some(l) = f.lines.iter().find(|l| l.atoms.iter().any(|a| self.atom_hit(a, c)) && self.cond_holds(&l.cond, reg, reg)) {
                if let Some(v) = self.values[fi].iter().position(|v| *v == l.value) {
                    out[fi] = v;
                }
            }
        }
        out
    }

    fn run_left(&self, mut r: Reg, s: &str) -> Reg {
        for c in s.chars() {
            r = self.step(&r, c, false);
        }
        r
    }

    fn run_right(&self, mut r: Reg, s: &str) -> Reg {
        for c in s.chars().rev() {
            r = self.step(&r, c, true);
        }
        r
    }

    fn touches(&self, fi: usize, c: char) -> bool {
        self.spec.features[fi].lines.iter().any(|l| l.atoms.iter().any(|a| self.atom_hit(a, c)))
    }

    fn archi_lines(&self, name: &str) -> &'a [crate::spec::ArchiLine] {
        self.spec.archis.iter().find(|a| a.name == name).map_or(&[], |a| a.lines.as_slice())
    }

    fn reads_right(&self, name: &str) -> bool {
        self.archi_lines(name).iter().flat_map(|l| l.cond.iter().flatten()).any(|(f, _)| self.feat(f).is_some_and(|fi| self.spec.features[fi].right))
    }

    fn archi_outputs(&self, name: &str) -> Vec<&'a str> {
        let mut v: Vec<&str> = self.archi_lines(name).iter().map(|l| l.output.as_str()).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// First line whose condition holds.
    fn resolve(&self, name: &str, l: &Reg, r: &Reg) -> &'a str {
        self.archi_lines(name).iter().find(|ln| ln.cond.as_ref().is_none_or(|c| self.cond_holds(c, l, r))).map_or("", |ln| ln.output.as_str())
    }

    fn overlay(&self, reg: &Reg, pins: &Pins, list: &[usize]) -> Reg {
        let mut e = reg.clone();
        for (i, &f) in list.iter().enumerate() {
            if let Some(v) = pins[i] {
                e[f] = v;
            }
        }
        e
    }

    /// Next pins after a morpheme with final text `t` and flags.
    fn next_pins(&self, pins: &Pins, list: &[usize], t: &str, flags: &[String], prefix: &str) -> Pins {
        let mut p = pins.clone();
        for (i, &f) in list.iter().enumerate() {
            if t.chars().any(|c| self.touches(f, c)) {
                p[i] = None;
            }
        }
        for fl in flags {
            if let Some((feat, val)) = fl.strip_prefix(prefix).and_then(|kv| kv.split_once('=')) {
                let Some(fi) = self.feat(feat) else { continue };
                let Some(pi) = list.iter().position(|&x| x == fi) else { continue };
                p[pi] = self.values[fi].iter().position(|v| *v == val);
            }
        }
        p
    }

    /// Realise a template from effective left register `eff`. Left-only
    /// archiphonemes resolve; right-reading ones branch. `force` replaces
    /// the k-th archiphoneme's output (wrong-choice negatives).
    /// Returns (base text, checks) per branch.
    fn realize(&self, k_morph: usize, tpl: &[Sym], eff: &Reg, force: Option<(usize, &str)>) -> Vec<(String, Vec<Check>)> {
        // partial: (text, left register, checks with suffix start offsets)
        let mut partial: Vec<Partial> = vec![(String::new(), eff.clone(), vec![])];
        let zero = self.zero();
        let mut k = 0;
        for s in tpl {
            let mut next = Vec::new();
            for (text, reg, checks) in partial {
                // (output, archiphoneme name when the choice is branched)
                let outs: Vec<(String, Option<&String>)> = match s {
                    Sym::Ch(c) => vec![(c.to_string(), None)],
                    Sym::Archi(a) => match force {
                        Some((j, f)) if j == k => vec![(f.to_string(), None)],
                        _ if self.reads_right(a) => self.archi_outputs(a).into_iter().map(|o| (o.to_string(), Some(a))).collect(),
                        _ => vec![(self.resolve(a, &reg, &zero).to_string(), None)],
                    },
                };
                for (o, branched) in outs {
                    let mut t = text.clone();
                    let mut c2 = checks.clone();
                    t.push_str(&o);
                    if let Some(a) = branched {
                        c2.push((Check { morph: k_morph, archi: a.clone(), chosen: o.clone(), left: reg.clone(), suffix: String::new() }, t.len()));
                    }
                    next.push((t, self.run_left(reg.clone(), &o), c2));
                }
            }
            if matches!(s, Sym::Archi(_)) {
                k += 1;
            }
            partial = next;
        }
        partial
            .into_iter()
            .map(|(t, _, checks)| {
                let cs = checks.into_iter().map(|(mut c, at)| {
                    c.suffix = t[at..].to_string();
                    c
                });
                (t.clone(), cs.collect())
            })
            .collect()
    }

    /// Verify the branched choices of a complete realisation.
    fn verify(&self, path: &[&SpecEntry], pieces: &[String], checks: &[Check]) -> bool {
        // `pieces` are the texts the registers read (surface or tiers)
        if checks.is_empty() {
            return true;
        }
        // effective right register at each morpheme's end, right to left
        let n = pieces.len();
        let mut eff_end: Vec<Reg> = vec![self.zero(); n];
        let mut rtrue = self.zero();
        let mut pins: Pins = vec![None; self.rpins.len()];
        for k in (0..n).rev() {
            eff_end[k] = self.overlay(&rtrue, &pins, &self.rpins);
            rtrue = self.run_right(rtrue, &pieces[k]);
            pins = self.next_pins(&pins, &self.rpins, &pieces[k], &path[k].flags, "rset:");
        }
        checks.iter().all(|c| {
            let r = self.run_right(eff_end[c.morph].clone(), &c.suffix);
            self.resolve(&c.archi, &c.left, &r) == c.chosen
        })
    }
    /// Own rule matcher: rightmost target position whose suffix matches the
    /// tail (NFA simulation) and whose left context holds.
    fn rule_apply(&self, rule: &SpecRule, s: &str) -> Option<String> {
        let chars: Vec<char> = s.chars().collect();
        for st in (0..chars.len()).rev() {
            let tlen = match &rule.target {
                Target::AnyChar => 1,
                Target::Class(n) => {
                    if !self.in_class(n, chars[st]) {
                        continue;
                    }
                    1
                }
                Target::Literal(t) => {
                    let tc: Vec<char> = t.chars().collect();
                    if chars.len() < st + tc.len() || chars[st..st + tc.len()] != tc[..] {
                        continue;
                    }
                    tc.len()
                }
            };
            // NFA over tail atoms: set of atom indices reachable.
            let after = &chars[st + tlen..];
            let n = rule.tail.len();
            let eps = |set: &mut BTreeSet<usize>| loop {
                let add: Vec<usize> =
                    set.iter().filter(|&&i| i < n && matches!(rule.tail[i].1, Quant::Star | Quant::Opt)).map(|&i| i + 1).filter(|j| !set.contains(j)).collect();
                if add.is_empty() {
                    break;
                }
                set.extend(add);
            };
            let mut cur: BTreeSet<usize> = BTreeSet::from([0]);
            eps(&mut cur);
            for &c in after {
                let mut nxt = BTreeSet::new();
                for &i in &cur {
                    if i < n && self.atom_hit(&rule.tail[i].0, c) {
                        match rule.tail[i].1 {
                            Quant::One | Quant::Opt => {
                                nxt.insert(i + 1);
                            }
                            Quant::Plus | Quant::Star => {
                                nxt.insert(i);
                                nxt.insert(i + 1);
                            }
                        }
                    }
                }
                eps(&mut nxt);
                cur = nxt;
            }
            if !cur.contains(&n) {
                continue;
            }
            let pre: String = chars[..st].iter().collect();
            let last = chars[..st].last().copied();
            let ok = match &rule.left {
                LeftCtx::Any => true,
                LeftCtx::Boundary => st == 0,
                LeftCtx::Consonant => last.is_some_and(|c| self.in_class("C", c)),
                LeftCtx::Vowel => last.is_some_and(|c| self.in_class("V", c)),
                LeftCtx::Class(n) => last.is_some_and(|c| self.in_class(n, c)),
                LeftCtx::Literal(l) => pre.ends_with(l.as_str()),
            };
            if !ok {
                continue;
            }
            let matched: String = chars[st..st + tlen].iter().collect();
            let rest: String = chars[st + tlen..].iter().collect();
            return Some(format!("{pre}{}{rest}", rule.replacement.replace('$', &matched)));
        }
        None
    }

    fn right_ok(&self, rule: &SpecRule, next: &SpecEntry) -> bool {
        match &rule.right {
            RightCtx::Lex(v) => v.contains(&next.surface),
            RightCtx::Class(n) => parse_template(&next.surface).ok().and_then(|t| t.into_iter().next()).is_some_and(|s| self.sym_in_class(n, &s)),
        }
    }

    /// All realisations of a path: (per-morpheme strings, verified?).
    fn realize_path(&self, path: &[&SpecEntry], rules_on: bool) -> Vec<(Vec<String>, Vec<String>, bool)> {
        let mut out = Vec::new();
        let mut stack: Vec<Frame> = vec![(0, self.zero(), vec![None; self.lpins.len()], vec![], vec![], vec![])];
        let underlying = self.spec.options.underlying;
        while let Some((k, reg, pins, pieces, tiers, checks)) = stack.pop() {
            if k == path.len() {
                let ok = self.verify(path, &tiers, &checks);
                out.push((pieces, tiers, ok));
                continue;
            }
            let e = path[k];
            let next = path.get(k + 1).copied();
            if let Some(cls) = e.flags.iter().find_map(|f| f.strip_prefix("next=")) {
                match next {
                    Some(n) if parse_template(&n.surface).ok().and_then(|t| t.into_iter().next()).is_some_and(|s| self.sym_in_class(cls, &s)) => {}
                    _ => continue,
                }
            }
            let Ok(tpl) = parse_template(&e.surface) else { continue };
            let eff = self.overlay(&reg, &pins, &self.lpins);
            let tier_override = e.flags.iter().find_map(|f| f.strip_prefix("tier="));
            for (base, new_checks) in self.realize(k, &tpl, &eff, None) {
                let tier: Option<String> = underlying.then(|| tier_override.map(String::from).unwrap_or_else(|| base.clone()));
                let mut cands = vec![base];
                if rules_on {
                    if let Some(nx) = next {
                        for rule in &self.spec.rules {
                            if rule.require_flag.as_ref().is_some_and(|f| !e.flags.contains(f)) || !self.right_ok(rule, nx) {
                                continue;
                            }
                            let mut nc = Vec::new();
                            for c in &cands {
                                match self.rule_apply(rule, c) {
                                    Some(t) => {
                                        nc.push(t);
                                        if rule.optional {
                                            nc.push(c.clone());
                                        }
                                    }
                                    None => nc.push(c.clone()),
                                }
                            }
                            nc.sort();
                            nc.dedup();
                            cands = nc;
                        }
                    }
                }
                for t in cands {
                    let rt = tier.clone().unwrap_or_else(|| t.clone());
                    let r2 = self.run_left(reg.clone(), &rt);
                    let p2 = self.next_pins(&pins, &self.lpins, &rt, &e.flags, "set:");
                    let mut pc = pieces.clone();
                    pc.push(t);
                    let mut tc = tiers.clone();
                    tc.push(rt);
                    let mut ch = checks.clone();
                    ch.extend(new_checks.iter().cloned());
                    stack.push((k + 1, r2, p2, pc, tc, ch));
                }
            }
        }
        out
    }

    /// Strings where exactly one left-reading archiphoneme of one morpheme
    /// is realised with a different output (other morphemes as in `real`).
    /// (Wrong right-reading choices are the rejected branches.)
    fn wrong_archis(&self, path: &[&SpecEntry], real: &[String], regtext: &[String], out: &mut BTreeSet<String>) {
        let mut reg = self.zero();
        let zero = self.zero();
        let mut pins: Pins = vec![None; self.lpins.len()];
        for (k, e) in path.iter().enumerate() {
            let Ok(tpl) = parse_template(&e.surface) else { return };
            let eff = self.overlay(&reg, &pins, &self.lpins);
            let mut j = 0;
            let mut r = eff.clone();
            for s in &tpl {
                match s {
                    Sym::Ch(c) => r = self.step(&r, *c, false),
                    Sym::Archi(a) => {
                        if !self.reads_right(a) {
                            let chosen = self.resolve(a, &r, &zero);
                            for alt in self.archi_outputs(a) {
                                if alt != chosen {
                                    for (wrong, _) in self.realize(k, &tpl, &eff, Some((j, alt))) {
                                        let mut s: String = real[..k].concat();
                                        s.push_str(&wrong);
                                        s.push_str(&real[k + 1..].concat());
                                        out.insert(s);
                                    }
                                }
                            }
                            r = self.run_left(r, chosen);
                        } else {
                            // its realised output is unknown here; stop
                            // generating left-only negatives for this morpheme
                            break;
                        }
                        j += 1;
                    }
                }
            }
            reg = self.run_left(reg, &regtext[k]);
            pins = self.next_pins(&pins, &self.lpins, &regtext[k], &e.flags, "set:");
        }
    }
}

/// Enumerate all paths of at most `max_morphs` morphemes (up to
/// `max_paths` complete paths) and realise them. A spec that does not
/// compile is an error.
pub fn generate(spec: &Spec, max_morphs: usize, max_paths: usize) -> Result<Generated> {
    crate::spec::compile_grammar(spec)?;
    let it = Interp::new(spec);
    let lex: HashMap<&str, &[SpecEntry]> = spec.lexicons.iter().map(|l| (l.name.as_str(), l.entries.as_slice())).collect();
    let mut out = Generated::default();
    // (class, overt morphemes, gloss including zero morphemes)
    let mut stack: Vec<(&str, Vec<&SpecEntry>, Gloss)> = vec![("Root", Vec::new(), Vec::new())];
    let mut paths = 0usize;
    while let Some((class, path, gloss)) = stack.pop() {
        if class == "#" {
            if path.is_empty() {
                continue;
            }
            paths += 1;
            if paths > max_paths {
                break;
            }
            let reals = it.realize_path(&path, true);
            for (real, _, ok) in &reals {
                if *ok {
                    out.surfaces.entry(real.concat()).or_default().insert(gloss.clone());
                } else if out.wrong_archi.len() < 200_000 {
                    out.wrong_archi.insert(real.concat());
                }
            }
            for (naive, _, _) in it.realize_path(&path, false) {
                out.naive.insert(naive.concat());
            }
            if let Some((first, tiers, _)) = reals.iter().find(|(_, _, ok)| *ok) {
                if out.wrong_archi.len() < 200_000 {
                    it.wrong_archis(&path, first, tiers, &mut out.wrong_archi);
                }
            }
            continue;
        }
        let Some(entries) = lex.get(class) else { continue };
        for e in entries.iter().rev() {
            let mut g = gloss.clone();
            if e.is_zero() {
                g.push((String::new(), e.tag.clone()));
                stack.push((&e.cont, path.clone(), g));
            } else if e.is_epsilon() {
                stack.push((&e.cont, path.clone(), g));
            } else if path.len() < max_morphs {
                let mut p = path.clone();
                p.push(e);
                g.push((e.lemma.clone(), e.tag.clone()));
                stack.push((&e.cont, p, g));
            }
        }
    }
    Ok(out)
}
