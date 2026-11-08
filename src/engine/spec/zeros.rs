//! Zero morphemes (`0 TAG Cont ;`): morphemes with an empty surface, such
//! as the English singular or infinitive.
//!
//! The lattice has only non-empty arcs, so zeros are folded away before
//! compilation: every morpheme a zero can follow gets a copy that continues
//! where the zero continues and records the zero's tag. Rules, filters and
//! harmony see exactly what they would see without the zero.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use super::ast::{Spec, SpecEntry};
use crate::error::{spec_err, Result};

/// `spec` with its zero morphemes folded into the morphemes before them.
pub fn fold(spec: &Spec) -> Result<Cow<'_, Spec>> {
    if !spec.lexicons.iter().any(|l| l.entries.iter().any(SpecEntry::is_zero)) {
        return Ok(Cow::Borrowed(spec));
    }
    let index: BTreeMap<&str, usize> = spec.lexicons.iter().enumerate().map(|(i, l)| (l.name.as_str(), i)).collect();
    let mut zeros: Vec<Vec<(usize, &SpecEntry)>> = vec![Vec::new(); spec.lexicons.len()];
    let mut n = 0;
    for (i, l) in spec.lexicons.iter().enumerate() {
        for e in l.entries.iter().filter(|e| e.is_zero()) {
            zeros[i].push((n, e));
            n += 1;
        }
    }
    // Lexicons reachable from `start` through plain epsilons.
    let closure = |start: &str| -> Vec<usize> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![start];
        while let Some(name) = stack.pop() {
            let Some(&i) = index.get(name) else { continue };
            if seen.insert(i) {
                stack.extend(spec.lexicons[i].entries.iter().filter(|e| e.is_epsilon() && !e.is_zero()).map(|e| e.cont.as_str()));
            }
        }
        seen.into_iter().collect()
    };
    if let Some(&(_, z)) = closure("Root").iter().find_map(|&l| zeros[l].first()) {
        return spec_err(z.line, "a zero morpheme must follow a morpheme");
    }
    let mut out = spec.clone();
    for l in &mut out.lexicons {
        l.entries.retain(|e| !e.is_zero());
    }
    // (lexicon, morpheme, zero ids already folded into it)
    let mut work: Vec<(usize, SpecEntry, Vec<usize>)> = spec
        .lexicons
        .iter()
        .enumerate()
        .flat_map(|(i, l)| l.entries.iter().filter(|e| !e.is_epsilon()).map(move |e| (i, e.clone(), Vec::new())))
        .collect();
    work.reverse();
    let mut reach: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    while let Some((lex, e, used)) = work.pop() {
        let ls = reach.entry(e.cont.clone()).or_insert_with(|| closure(&e.cont)).clone();
        for l in ls {
            for &(id, z) in &zeros[l] {
                if used.contains(&id) {
                    return spec_err(z.line, "zero morphemes form a cycle");
                }
                let mut copy = e.clone();
                copy.cont.clone_from(&z.cont);
                copy.zeros.push(z.tag.clone());
                let mut u = used.clone();
                u.push(id);
                if let Some(target) = out.lexicons.get_mut(lex) {
                    target.entries.push(copy.clone());
                }
                work.push((lex, copy, u));
            }
        }
    }
    Ok(Cow::Owned(out))
}
