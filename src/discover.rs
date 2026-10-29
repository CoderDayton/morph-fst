//! Draft a `.morph` spec from a plain word list: no labels, no tables.
//!
//! Paradigm discovery in the style of Goldsmith's Linguistica, kept
//! deterministic and parameter-light:
//!
//! * every word is cut at every point into stem (at least 3 letters) and
//!   suffix (at most 10, possibly empty);
//! * a suffix is **productive** when enough stems (at least
//!   `max(3, words / 1000)`) carry it beside another suffix;
//! * each word keeps the cut whose stem carries the most productive
//!   suffixes, the longest such stem on a tie. `walk` beats `wal` for
//!   `walked` because `walk` is also a word;
//! * a suffix that is a concatenation of shorter productive ones
//!   (`larda` = `lar` + `da`) is split into them, and endings chain through
//!   a shared tail, so agglutinative stacks are not memorized whole;
//! * stems with the same suffix set share a paradigm class; a stem seen
//!   with one suffix only is not evidence, so its words are listed whole.
//!
//! The suffixes carry no features, only their spelling (`SFX:ed`). The
//! point is the stem classes: hybrid mode borrows endings from them, so
//! a language gets a usable first grammar from raw text alone.

use crate::error::{Error, Result};
use crate::generate::{clean, ident};
use crate::normalize::{normalize, NormOptions};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

const MIN_STEM: usize = 3;
const MAX_SUFFIX: usize = 10;
const MIN_SUPPORT: usize = 3;
const MIN_PIECE: usize = 2;
const MERGE_OVERLAP: usize = 10;

/// A discovered spec and what went into it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Discovered {
    /// The `.morph` source. It is checked to compile before it is returned.
    pub spec: String,
    /// Distinct usable words.
    pub words: usize,
    /// Productive suffixes the paradigms use, sorted; the empty suffix is left out.
    pub suffixes: Vec<String>,
    /// Paradigm classes (distinct suffix sets).
    pub paradigms: usize,
    /// Words listed whole because no stem had evidence.
    pub whole: usize,
}

/// Build a spec from whitespace-separated words. Input with no usable
/// words is an error.
pub fn from_wordlist(text: &str) -> Result<Discovered> {
    discover_named(text, "")
}

/// [`from_wordlist`] with `prefix` on every lexicon name, so the result
/// can be spliced into another spec.
pub(crate) fn discover_named(text: &str, prefix: &str) -> Result<Discovered> {
    let norm = NormOptions::default();
    let words: BTreeSet<String> = text
        .split_whitespace()
        .map(|w| normalize(w, &norm))
        .filter(|w| !w.is_empty() && w.chars().all(char::is_alphabetic))
        .collect();
    if words.is_empty() {
        return Err(Error::Spec { line: 0, msg: "no usable words".into() });
    }
    let cuts: Vec<(&str, Vec<(&str, &str)>)> = words.iter().map(|w| (w.as_str(), splits(w))).collect();

    let mut carried: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (_, parts) in &cuts {
        for &(stem, sfx) in parts {
            carried.entry(stem).or_default().insert(sfx);
        }
    }
    let mut support: BTreeMap<&str, usize> = BTreeMap::new();
    for sfxs in carried.values().filter(|s| s.len() > 1) {
        for &s in sfxs {
            *support.entry(s).or_default() += 1;
        }
    }
    let need = MIN_SUPPORT.max(words.len() / 1000);
    let productive = |s: &str| support.get(s).is_some_and(|&n| n >= need);

    let mut chosen: BTreeMap<&str, (BTreeSet<&str>, Vec<&str>)> = BTreeMap::new();
    let mut whole: Vec<&str> = Vec::new();
    for (word, parts) in &cuts {
        let best = parts
            .iter()
            .filter(|&&(_, sfx)| productive(sfx))
            .max_by_key(|&&(stem, _)| (carried[stem].iter().filter(|&&s| productive(s)).count(), stem.len()));
        match best {
            Some(&(stem, sfx)) => {
                let e = chosen.entry(stem).or_default();
                e.0.insert(sfx);
                e.1.push(word);
            }
            None => whole.push(word),
        }
    }
    let used: BTreeSet<&str> = chosen.values().filter(|(s, _)| s.len() > 1).flat_map(|(s, _)| s.iter().copied()).filter(|s| !s.is_empty()).collect();
    let chains = chains(&used);
    let mut classes: BTreeMap<Vec<Ending>, Vec<&str>> = BTreeMap::new();
    for (stem, (sfxs, ws)) in chosen {
        if sfxs.len() > 1 {
            let mut sig: BTreeMap<&str, (bool, bool)> = BTreeMap::new();
            for s in sfxs {
                match chains.get(s) {
                    Some(p) => sig.entry(p[0]).or_default().1 = true,
                    None => sig.entry(s).or_default().0 = true,
                }
            }
            let sig = sig.into_iter().map(|(s, (ends, goes_on))| Ending { sfx: s, ends, goes_on }).collect();
            classes.entry(sig).or_default().push(stem);
        } else {
            whole.extend(ws);
        }
    }
    whole.sort_unstable();
    let mut classes: Vec<_> = classes.into_iter().collect();
    classes.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));

    let suffixes: BTreeSet<&str> = used.iter().copied().filter(|s| !chains.contains_key(s)).collect();
    let tail: BTreeSet<&str> = chains.values().flatten().copied().collect();
    let tags = variants(&used, &carried, &support);
    let spec = render(&words, &classes, &whole, &tail, &tags, prefix);
    crate::spec::load(&spec).map_err(|e| match e {
        Error::Spec { line, msg } => Error::Spec { line, msg: format!("discovered spec does not compile: {msg}") },
        e => e,
    })?;
    Ok(Discovered {
        spec,
        words: words.len(),
        suffixes: suffixes.into_iter().map(String::from).collect(),
        paradigms: classes.len(),
        whole: whole.len(),
    })
}

/// One ending a paradigm class offers: it may end the word, continue into
/// the shared tail, or both.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Ending<'a> {
    sfx: &'a str,
    ends: bool,
    goes_on: bool,
}

/// Spellings that differ only in their vowels (`lar`/`ler`) and never share
/// a stem are one suffix under vowel harmony. Maps each to the best-supported
/// spelling of its group, which gives them one tag.
fn variants<'a>(used: &BTreeSet<&'a str>, carried: &BTreeMap<&str, BTreeSet<&str>>, support: &BTreeMap<&str, usize>) -> BTreeMap<&'a str, &'a str> {
    let mut groups: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for &s in used {
        groups.entry(s.chars().map(|c| if is_vowel(c) { '_' } else { c }).collect()).or_default().push(s);
    }
    groups.retain(|k, g| g.len() > 1 && k.contains('_'));
    let grouped: BTreeSet<&str> = groups.values().flatten().copied().collect();
    let mut both: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for sfxs in carried.values() {
        let hits: Vec<&str> = sfxs.iter().copied().filter(|s| grouped.contains(s)).collect();
        for (i, &a) in hits.iter().enumerate() {
            for &b in hits.iter().skip(i + 1) {
                *both.entry((a, b)).or_default() += 1;
            }
        }
    }
    let n = |s: &str| support.get(s).copied().unwrap_or(0);
    let mut out = BTreeMap::new();
    for mut group in groups.into_values() {
        group.sort_by(|a, b| n(b).cmp(&n(a)).then_with(|| a.cmp(b)));
        let mut heads: Vec<&str> = Vec::new();
        for s in group {
            let apart = |h: &&str| {
                let (a, b) = if h < &s { (*h, s) } else { (s, *h) };
                both.get(&(a, b)).copied().unwrap_or(0) * MERGE_OVERLAP <= n(s).min(n(h))
            };
            let head = heads.iter().copied().find(apart);
            match head {
                Some(h) => {
                    out.insert(s, h);
                }
                None => heads.push(s),
            }
        }
    }
    out
}

fn is_vowel(c: char) -> bool {
    "aeiouyäöüıåæøáàâéèêíìîóòôúùû".contains(c)
}

/// Suffixes that are a concatenation of shorter ones, with their fewest pieces.
fn chains<'a>(all: &BTreeSet<&'a str>) -> BTreeMap<&'a str, Vec<&'a str>> {
    let mut by_len: Vec<&str> = all.iter().copied().collect();
    by_len.sort_by_key(|s| (s.chars().count(), *s));
    let mut atoms: Vec<&str> = Vec::new();
    let mut out = BTreeMap::new();
    for s in by_len {
        match pieces(s, &atoms) {
            Some(p) if p.len() > 1 => {
                out.insert(s, p);
            }
            _ if s.chars().count() >= MIN_PIECE => atoms.push(s),
            _ => {}
        }
    }
    out
}

fn pieces<'a>(s: &str, atoms: &[&'a str]) -> Option<Vec<&'a str>> {
    let mut best: BTreeMap<usize, Vec<&'a str>> = BTreeMap::new();
    best.insert(0, Vec::new());
    for (i, c) in s.char_indices() {
        let end = i + c.len_utf8();
        let mut pick: Option<Vec<&str>> = None;
        for &a in atoms {
            let Some(start) = end.checked_sub(a.len()) else { continue };
            if s.get(start..end) != Some(a) {
                continue;
            }
            if let Some(prev) = best.get(&start) {
                if pick.as_ref().is_none_or(|p| prev.len() + 1 < p.len()) {
                    let mut p = prev.clone();
                    p.push(a);
                    pick = Some(p);
                }
            }
        }
        if let Some(p) = pick {
            best.insert(end, p);
        }
    }
    best.remove(&s.len())
}

/// Every (stem, suffix) cut of `word` with a long-enough stem and a short-enough suffix.
fn splits(word: &str) -> Vec<(&str, &str)> {
    let mut at: Vec<usize> = word.char_indices().map(|(i, _)| i).collect();
    at.push(word.len());
    let n = at.len() - 1;
    (MIN_STEM..=n).filter(|&i| n - i <= MAX_SUFFIX).map(|i| word.split_at(at[i])).collect()
}

fn render(words: &BTreeSet<String>, classes: &[(Vec<Ending>, Vec<&str>)], whole: &[&str], tail: &BTreeSet<&str>, tags: &BTreeMap<&str, &str>, p: &str) -> String {
    let tag_of = |s: &str| ident(tags.get(s).copied().unwrap_or(s));
    let mut out = String::new();
    out.push_str("! Generated by `morph-fst discover` from a plain word list.\n");
    let _ = writeln!(out, "! {} words, {} paradigm classes, {} words listed whole.", words.len(), classes.len(), whole.len());
    out.push_str("! Suffixes carry no features; stems with the same suffix set share a class.\n\n");
    out.push_str("LEXICON Root\n");
    if !classes.is_empty() {
        let _ = writeln!(out, "  0 {p}Stems ;");
    }
    if !whole.is_empty() {
        let _ = writeln!(out, "  0 {p}Whole ;");
    }
    if !classes.is_empty() {
        let _ = writeln!(out, "\nLEXICON {p}Stems @open=STEM");
        for (i, (_, stems)) in classes.iter().enumerate() {
            for stem in stems {
                let _ = writeln!(out, "  {stem} STEM {p}Class_{} ;", i + 1);
            }
        }
    }
    if !whole.is_empty() {
        let _ = writeln!(out, "\nLEXICON {p}Whole");
        for w in whole {
            let _ = writeln!(out, "  {w} WORD # ;");
        }
    }
    for (i, (sig, stems)) in classes.iter().enumerate() {
        let shown: Vec<&str> = stems.iter().take(5).copied().collect();
        let more = if stems.len() > 5 { ", …" } else { "" };
        let _ = writeln!(out, "\nLEXICON {p}Class_{}    ! {} stem(s): {}{more}", i + 1, stems.len(), clean(&shown.join(", ")));
        for e in sig {
            let (surface, tag) = if e.sfx.is_empty() { ("0", "0".to_string()) } else { (e.sfx, tag_of(e.sfx)) };
            if e.ends {
                let _ = writeln!(out, "  {surface} SFX:{tag} # ;");
            }
            if e.goes_on {
                let _ = writeln!(out, "  {surface} SFX:{tag} {p}Tail ;");
            }
        }
    }
    if !tail.is_empty() {
        let _ = writeln!(out, "\nLEXICON {p}Tail    ! endings that stack");
        for a in tail {
            let _ = writeln!(out, "  {a} SFX:{t} # ;\n  {a} SFX:{t} {p}Tail ;", t = tag_of(a));
        }
    }
    out
}
