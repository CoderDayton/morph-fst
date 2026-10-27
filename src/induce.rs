//! Learning lexical exceptions from raw text (no hand marking).
//!
//! A spec declares *where* exceptions may occur:
//!
//! ```text
//! INDUCE vow IN Nouns Adjs        ! roots may carry their own value of `vow`
//! ```
//!
//! [`induce`] then decides *which* roots are exceptional from a corpus:
//!
//! 1. Build a hypothesis grammar in which every root of an INDUCE lexicon
//!    exists once per hypothesis: its surface behaviour, and each single
//!    value `f=v` of each inducible feature (as `@set:f=v` for left
//!    FEATUREs, `@rset:f=v` for RFEATUREs).
//! 2. Analyse every corpus word type. For each root an analysis uses, the
//!    hypotheses that explain the word get the word's frequency as support
//!    — unless *every* hypothesis explains it (uninformative).
//! 3. A root keeps its surface behaviour unless some exception has strictly
//!    more support than the surface reading and at least
//!    `OPTION induce_min_count` tokens. Ties between exceptions go to the
//!    first declared value and are reported as ambiguous.
//!
//! Roots that already carry a hand-written `@set`/`@rset` for a feature are
//! left alone for that feature. The result is an ordinary spec, so the
//! compiled lexer is exactly what hand marking would have produced.

use std::collections::BTreeMap;

use crate::error::Result;
use crate::spec::{self, Spec};

/// Marker separating a lemma from its hypothesis tag in the hypothesis grammar.
const MARK: char = '\u{1F}';

/// One (root, feature) decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub lexicon: String,
    /// Lexical template of the root, e.g. `saat`.
    pub surface: String,
    pub lemma: String,
    /// The feature decided.
    pub feature: String,
    /// Chosen flag, e.g. `set:vow=fu` (None = surface behaviour).
    pub flag: Option<String>,
    /// Support (tokens) for the chosen hypothesis and for the surface one.
    pub support: u64,
    pub surface_support: u64,
    /// Other hypotheses with support, best first.
    pub alternatives: Vec<(String, u64)>,
    /// Several exceptions tied for best.
    pub ambiguous: bool,
}

/// Result of [`induce`].
#[derive(Clone, Debug)]
pub struct Induction {
    /// The spec with learned flags added (compile it as usual).
    pub spec: Spec,
    /// Every root with any informative evidence, plus every exception.
    pub decisions: Vec<Decision>,
    /// Corpus size used.
    pub tokens: u64,
    pub types: usize,
}

impl Induction {
    /// Only the roots that became exceptions.
    pub fn exceptions(&self) -> impl Iterator<Item = &Decision> {
        self.decisions.iter().filter(|d| d.flag.is_some())
    }

    /// The original spec text with the learned flags written in, so the
    /// result can be reviewed and checked in.
    pub fn annotate(&self, spec_text: &str) -> String {
        let mut by_line: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for lex in &self.spec.lexicons {
            for e in &lex.entries {
                for d in &self.decisions {
                    let Some(flag) = &d.flag else { continue };
                    if d.lexicon == lex.name && d.surface == e.surface && e.flags.contains(flag) {
                        by_line.entry(e.line).or_default().push(flag.clone());
                    }
                }
            }
        }
        let mut out = String::new();
        for (i, line) in spec_text.lines().enumerate() {
            match by_line.get(&(i + 1)) {
                Some(flags) => {
                    let (body, comment) = match line.find('!') {
                        Some(k) => (&line[..k], &line[k..]),
                        None => (line, ""),
                    };
                    let body = body.trim_end();
                    let core = body.strip_suffix(';').unwrap_or(body).trim_end();
                    let extra: Vec<String> = flags.iter().map(|f| format!("@{f}")).collect();
                    out.push_str(&format!("{core} {} ;  ! learned{}", extra.join(" "), if comment.is_empty() { String::new() } else { format!(" {comment}") }));
                }
                None => out.push_str(line),
            }
            out.push('\n');
        }
        out
    }
}

struct Root {
    lexicon: String,
    li: usize,
    ei: usize,
    /// hypotheses: index 0 = surface, then (flag) per feature value
    hyps: Vec<Option<String>>,
    /// feature name of each hypothesis (index 0: unused)
    group: Vec<String>,
}

/// Learn exceptions for `spec` from raw `corpus` text.
pub fn induce(spec: &Spec, corpus: &str) -> Result<Induction> {
    let mut out = Induction { spec: spec.clone(), decisions: Vec::new(), tokens: 0, types: 0 };
    if spec.induce.is_empty() {
        return Ok(out);
    }
    // inducible (prefix, feature, values)
    let mut feats: Vec<(&str, String, Vec<String>)> = Vec::new();
    for d in &spec.induce {
        for f in &d.features {
            let Some(def) = spec.features.iter().find(|x| &x.name == f) else { continue };
            let mut vals: Vec<String> = Vec::new();
            for l in &def.lines {
                if !vals.contains(&l.value) {
                    vals.push(l.value.clone());
                }
            }
            let prefix = if def.right { "rset:" } else { "set:" };
            if !feats.iter().any(|(_, n, _)| n == f) {
                feats.push((prefix, f.clone(), vals));
            }
        }
    }
    // roots and their hypotheses
    let mut roots: Vec<Root> = Vec::new();
    for (li, lex) in spec.lexicons.iter().enumerate() {
        let feats_here: Vec<&(&str, String, Vec<String>)> = feats
            .iter()
            .filter(|(_, f, _)| spec.induce.iter().any(|d| d.features.contains(f) && d.lexicons.contains(&lex.name)))
            .collect();
        if feats_here.is_empty() {
            continue;
        }
        for (ei, e) in lex.entries.iter().enumerate() {
            if e.is_epsilon() {
                continue;
            }
            let mut hyps = vec![None];
            let mut group = vec![String::new()];
            for (prefix, f, vals) in &feats_here {
                let marked = e.flags.iter().any(|x| x.starts_with(&format!("{prefix}{f}=")));
                if marked {
                    continue; // hand-written: keep
                }
                for v in vals {
                    hyps.push(Some(format!("{prefix}{f}={v}")));
                    group.push(f.clone());
                }
            }
            if hyps.len() > 1 {
                roots.push(Root { lexicon: lex.name.clone(), li, ei, hyps, group });
            }
        }
    }
    // hypothesis grammar: lemma carries "MARK root MARK hyp"
    let mut hspec = spec.clone();
    for (ri, r) in roots.iter().enumerate() {
        let base = hspec.lexicons[r.li].entries[r.ei].clone();
        let mut variants = Vec::new();
        for (hi, h) in r.hyps.iter().enumerate() {
            let mut e = base.clone();
            e.lemma = format!("{}{MARK}{ri}{MARK}{hi}", base.lemma);
            if let Some(flag) = h {
                e.flags.push(flag.clone());
            }
            variants.push(e);
        }
        hspec.lexicons[r.li].entries[r.ei] = variants.remove(0);
        hspec.lexicons[r.li].entries.extend(variants);
    }
    let lexer = crate::Lexer::from_grammar(&spec::compile_grammar(&hspec)?);
    // corpus word types
    let norm = lexer.mt.norm;
    let text = crate::normalize::normalize(corpus, &norm);
    let mut counts: BTreeMap<&str, u64> = BTreeMap::new();
    for (s, e) in crate::pretok::split(&text, norm.apostrophe_in_words) {
        *counts.entry(&text[s..e]).or_default() += 1;
        out.tokens += 1;
    }
    out.types = counts.len();
    // support
    let mut support: Vec<Vec<u64>> = roots.iter().map(|r| vec![0; r.hyps.len()]).collect();
    for (w, &freq) in &counts {
        let Some(crate::WordOutput::Analyzed { all, .. }) = lexer.analyze(w, 4096) else { continue };
        let mut seen: BTreeMap<usize, Vec<bool>> = BTreeMap::new();
        for a in &all {
            for m in a {
                let mut it = m.lemma.split(MARK);
                let (Some(_), Some(ri), Some(hi)) = (it.next(), it.next(), it.next()) else { continue };
                let (Ok(ri), Ok(hi)) = (ri.parse::<usize>(), hi.parse::<usize>()) else { continue };
                seen.entry(ri).or_insert_with(|| vec![false; roots[ri].hyps.len()])[hi] = true;
            }
        }
        for (ri, hs) in seen {
            if hs.iter().all(|&x| x) {
                continue; // every hypothesis explains it: no information
            }
            for (hi, ok) in hs.iter().enumerate() {
                if *ok {
                    support[ri][hi] += freq;
                }
            }
        }
    }
    // decisions: each inducible feature of each root independently
    let min = spec.options.induce_min_count as u64;
    for (ri, r) in roots.iter().enumerate() {
        let sup = &support[ri];
        let surface = sup[0];
        let mut features: Vec<&String> = r.group[1..].iter().collect();
        features.dedup();
        for f in features {
            let members: Vec<usize> = (1..sup.len()).filter(|&h| &r.group[h] == f).collect();
            if surface == 0 && members.iter().all(|&h| sup[h] == 0) {
                continue;
            }
            let best = members.iter().map(|&h| sup[h]).max().unwrap_or(0);
            let winners: Vec<usize> = members.iter().copied().filter(|&h| sup[h] == best).collect();
            let pick = (best > surface && best >= min).then(|| winners[0]);
            let mut alternatives: Vec<(String, u64)> =
                members.iter().copied().filter(|&h| Some(h) != pick && sup[h] > 0).filter_map(|h| r.hyps[h].clone().map(|f| (f, sup[h]))).collect();
            alternatives.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            let e = &spec.lexicons[r.li].entries[r.ei];
            let flag = pick.and_then(|h| r.hyps[h].clone());
            if let Some(fl) = &flag {
                out.spec.lexicons[r.li].entries[r.ei].flags.push(fl.clone());
            }
            out.decisions.push(Decision {
                lexicon: r.lexicon.clone(),
                surface: e.surface.clone(),
                lemma: e.lemma.clone(),
                feature: f.clone(),
                flag,
                support: pick.map_or(surface, |h| sup[h]),
                surface_support: surface,
                alternatives,
                ambiguous: pick.is_some() && winners.len() > 1,
            });
        }
    }
    Ok(out)
}
