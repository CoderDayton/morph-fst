//! Generate a `.morph` spec from UniMorph inflection tables.
//!
//! Input is UniMorph TSV: one `lemma<TAB>form<TAB>FEAT;FEAT;…` row per
//! inflected form (<https://unimorph.github.io>). For each lemma and part
//! of speech:
//!
//! * the stem is the prefix of the lemma (letters compared without
//!   accents) after which most forms continue with a suffix that at least
//!   two lemmas show in the same cell when split at their common prefix;
//! * a form `stem + suffix` becomes a suffix entry tagged with the features
//!   other than the part of speech. An accented stem (`ór-ala` beside
//!   `or-ar`) is a stem variant;
//! * a form not starting with the stem (`piens-an` beside `pens-ar`) is
//!   split before the longest suffix that other lemmas show in the same
//!   cell, giving another stem variant;
//! * each stem variant's suffix set is a paradigm class, shared by every
//!   variant with the same set;
//! * forms equal to the stem, without features beyond the part of speech,
//!   or with no split (suppletion: `went`) are listed whole.
//!
//! The spec accepts exactly the input rows, each with its lemma and
//! features. Rules, harmony and archiphonemes are not inferred; the output
//! is a starting point to generalize by hand.

use crate::error::{Error, Result};
use crate::normalize::{base_letter, normalize, NormOptions};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

/// UniMorph part-of-speech features; the first one in a row is its POS.
const POS: &[&str] =
    &["N", "PROPN", "ADJ", "PRO", "CLF", "ART", "DET", "V", "V.PTCP", "V.MSDR", "V.CVB", "ADV", "AUX", "ADP", "COMP", "CONJ", "NUM", "PART", "INTJ"];
/// Open classes: hybrid mode may posit unknown stems of these.
const OPEN: &[&str] = &["N", "PROPN", "ADJ", "V", "ADV"];
/// Characters with meaning in `.morph` entry surfaces.
const RESERVED: &[char] = &['!', '#', ':', '{', '}', ';'];

/// A generated spec and what went into it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Generated {
    /// The `.morph` source. It is checked to compile before it is returned.
    pub spec: String,
    /// Distinct (lemma, part of speech) pairs.
    pub lemmas: usize,
    /// Distinct rows used.
    pub forms: usize,
    /// Suffix lexicons (paradigm classes).
    pub paradigms: usize,
    /// Forms listed whole because no stem or suffix split was found.
    pub irregular: usize,
    /// Rows left out: (1-based line, reason).
    pub skipped: Vec<(usize, String)>,
}

/// One form of a lemma: normalized surface and non-POS features.
type Cell = (String, String);
/// A suffix set shared by the lemmas of one paradigm class.
type Paradigm = BTreeSet<Cell>;
/// A stem-lexicon entry: surface, lemma, tag, continuation.
type StemEntry = (String, String, String, String);
/// A lemma's POS, label and stem variants with their suffix sets.
type LemmaSplit<'a> = (&'a str, &'a str, BTreeMap<String, Paradigm>);
/// (stem variant, lemma) pairs that use one paradigm.
type Users = Vec<(String, String)>;

/// Usable rows grouped by (POS, lemma), with the options they need.
struct Tables {
    groups: BTreeMap<(String, String), BTreeSet<Cell>>,
    norm: NormOptions,
    longest: usize,
    skipped: Vec<(usize, String)>,
}

/// Stem lexicons and paradigm classes, both keyed by POS.
#[derive(Default)]
struct Lexicons {
    stems: BTreeMap<String, BTreeSet<StemEntry>>,
    /// Classes in name order: (name, suffixes, lemmas using it).
    classes: BTreeMap<String, Vec<(String, Paradigm, Vec<String>)>>,
    forms: usize,
    irregular: usize,
}

/// Build a spec from UniMorph TSV. Unusable rows are skipped and reported;
/// input with no usable rows is an error.
pub fn from_unimorph(tsv: &str) -> Result<Generated> {
    let t = read_tables(tsv);
    if t.groups.is_empty() {
        return Err(Error::Spec { line: 0, msg: "no usable `lemma<TAB>form<TAB>features` rows".into() });
    }
    let lx = build(&t);
    let spec = render(&t, &lx);
    crate::spec::load(&spec).map_err(|e| match e {
        Error::Spec { line, msg } => Error::Spec { line, msg: format!("generated spec does not compile: {msg}") },
        e => e,
    })?;
    let paradigms = lx.classes.values().map(Vec::len).sum();
    Ok(Generated { spec, lemmas: t.groups.len(), forms: lx.forms, paradigms, irregular: lx.irregular, skipped: t.skipped })
}

fn read_tables(tsv: &str) -> Tables {
    let mut skipped = Vec::new();
    let mut rows = Vec::new();
    for (i, raw) in tsv.lines().enumerate() {
        if raw.trim().is_empty() {
            continue;
        }
        match parse_row(raw) {
            Ok(r) => rows.push((i + 1, r)),
            Err(why) => skipped.push((i + 1, why)),
        }
    }
    let hangul = rows.iter().any(|(_, (_, f, _))| f.chars().any(|c| ('\u{AC00}'..='\u{D7A3}').contains(&c)));
    let apostrophe = rows.iter().any(|(_, (_, f, _))| f.contains(['\'', '\u{2019}']));
    let norm = NormOptions { hangul_jamo: hangul, apostrophe_in_words: apostrophe, ..NormOptions::default() };
    let mut groups: BTreeMap<(String, String), BTreeSet<Cell>> = BTreeMap::new();
    let mut longest = 0;
    for (line, (lemma, raw_form, feats)) in rows {
        let form = normalize(raw_form, &norm);
        if form.chars().any(|c| c.is_whitespace() || RESERVED.contains(&c)) || form == "0" {
            skipped.push((line, format!("form `{raw_form}` contains a reserved character or space")));
            continue;
        }
        let pos_i = feats.iter().position(|f| POS.contains(f));
        let pos = pos_i.map_or("X", |i| feats[i]).to_string();
        let rest: Vec<&str> = feats.iter().enumerate().filter(|&(i, _)| Some(i) != pos_i).map(|(_, f)| *f).collect();
        longest = longest.max(form.chars().count());
        groups.entry((pos, lemma.to_string())).or_default().insert((form, rest.join(";")));
    }
    skipped.sort();
    Tables { groups, norm, longest, skipped }
}

/// `(lemma, form, features)` from one TSV row.
fn parse_row(raw: &str) -> std::result::Result<(&str, &str, Vec<&str>), String> {
    let cols: Vec<&str> = raw.split('\t').map(str::trim).collect();
    let (Some(&lemma), Some(&form), Some(&feats)) = (cols.first(), cols.get(1), cols.get(2)) else {
        return Err("expected `lemma<TAB>form<TAB>features`".into());
    };
    if lemma.is_empty() || form.is_empty() || feats.is_empty() {
        return Err("empty lemma, form or features".into());
    }
    if lemma.chars().any(|c| c.is_whitespace() || RESERVED.contains(&c)) {
        return Err(format!("lemma `{lemma}` contains a reserved character or space"));
    }
    Ok((lemma, form, feats.split(';').map(str::trim).filter(|f| !f.is_empty()).collect()))
}

fn build(t: &Tables) -> Lexicons {
    let mut lx = Lexicons::default();
    let mut splits: Vec<LemmaSplit> = Vec::new();
    let mut deferred: Vec<(usize, &str, &str)> = Vec::new();
    // (POS, features) -> suffix -> times seen after a stem
    let mut known: BTreeMap<(&str, &str), BTreeMap<&str, usize>> = BTreeMap::new();
    let seed = seed_suffixes(t);
    for ((pos, lemma), cells) in &t.groups {
        lx.forms += cells.len();
        let stem = choose_stem(&fold(&normalize(lemma, &t.norm)), pos, cells, &seed);
        let mut variants: BTreeMap<String, Paradigm> = BTreeMap::new();
        for (form, rest) in cells {
            match split_at_stem(form, &stem) {
                Some((v, sfx)) if !sfx.is_empty() && !rest.is_empty() => {
                    *known.entry((pos, rest)).or_default().entry(sfx).or_default() += 1;
                    variants.entry(v.to_string()).or_default().insert((sfx.to_string(), rest.clone()));
                }
                Some(_) => lx.whole(pos, lemma, form, rest),
                None if rest.is_empty() => {
                    lx.irregular += 1;
                    lx.whole(pos, lemma, form, rest);
                }
                None => deferred.push((splits.len(), form, rest)),
            }
        }
        splits.push((pos, lemma, variants));
    }
    for (i, form, rest) in deferred {
        let Some((pos, lemma, variants)) = splits.get_mut(i) else { continue };
        match split_before_known(form, known.get(&(*pos, rest))) {
            Some((v, sfx)) => {
                variants.entry(v.to_string()).or_default().insert((sfx.to_string(), rest.to_string()));
            }
            None => {
                lx.irregular += 1;
                lx.whole(pos, lemma, form, rest);
            }
        }
    }
    name_classes(&mut lx, splits);
    lx
}

/// One paradigm class per distinct suffix set, named per POS, most used
/// first; each stem variant points at its class.
fn name_classes(lx: &mut Lexicons, splits: Vec<LemmaSplit>) {
    // POS -> paradigm -> (stem, lemma) pairs using it
    let mut uses: BTreeMap<&str, BTreeMap<Paradigm, Users>> = BTreeMap::new();
    for (pos, lemma, variants) in splits {
        for (stem, paradigm) in variants {
            uses.entry(pos).or_default().entry(paradigm).or_default().push((stem, lemma.to_string()));
        }
    }
    for (pos, by_paradigm) in uses {
        let mut ranked: Vec<(Paradigm, Users)> = by_paradigm.into_iter().collect();
        ranked.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));
        let classes = lx.classes.entry(pos.to_string()).or_default();
        for (i, (paradigm, users)) in ranked.into_iter().enumerate() {
            let name = format!("{}_{}", ident(pos), i + 1);
            let stems = lx.stems.entry(pos.to_string()).or_default();
            for (stem, lemma) in &users {
                stems.insert((stem.clone(), lemma.clone(), pos.to_string(), name.clone()));
            }
            classes.push((name, paradigm, users.into_iter().map(|(_, l)| l).collect()));
        }
    }
}

impl Lexicons {
    /// List `form` whole in the stem lexicon, tagged with all its features.
    fn whole(&mut self, pos: &str, lemma: &str, form: &str, rest: &str) {
        let tag = if rest.is_empty() { pos.to_string() } else { format!("{pos};{rest}") };
        self.stems.entry(pos.to_string()).or_default().insert((form.to_string(), lemma.to_string(), tag, "#".into()));
    }
}

fn render(t: &Tables, lx: &Lexicons) -> String {
    let n_classes: usize = lx.classes.values().map(Vec::len).sum();
    let mut out = String::new();
    out.push_str("! Generated by `morph-fst generate` from UniMorph tables:\n");
    let _ = writeln!(out, "! {} lemmas, {} forms, {n_classes} paradigm classes, {} irregular forms.", t.groups.len(), lx.forms, lx.irregular);
    out.push_str("! Stem = longest lemma prefix (accents ignored) shared by at least half of its forms;\n");
    out.push_str("! other forms split before a suffix seen in the same cell, or are listed whole.\n");
    out.push_str("! Rules and harmony are not inferred; edit to generalize.\n\n");
    if t.norm.hangul_jamo {
        out.push_str("OPTION hangul_jamo true\n");
    }
    if t.norm.apostrophe_in_words {
        out.push_str("OPTION apostrophe_in_words true\n");
    }
    if t.longest > 64 {
        let _ = writeln!(out, "OPTION max_morph_chars {}", t.longest.min(4096));
    }
    out.push_str("\nLEXICON Root\n");
    for pos in lx.stems.keys() {
        let _ = writeln!(out, "  0 Stems_{} ;", ident(pos));
    }
    for (pos, entries) in &lx.stems {
        let open = if OPEN.contains(&pos.as_str()) { format!(" @open={pos}") } else { String::new() };
        let _ = writeln!(out, "\nLEXICON Stems_{}{open}", ident(pos));
        for (surface, lemma, tag, cont) in entries {
            let _ = writeln!(out, "  {} {} {cont} ;", entry_head(surface, lemma), clean(tag));
        }
    }
    for (name, paradigm, lemmas) in lx.classes.values().flatten() {
        let shown: Vec<&str> = lemmas.iter().take(5).map(String::as_str).collect();
        let more = if lemmas.len() > 5 { ", …" } else { "" };
        let _ = writeln!(out, "\nLEXICON {name}    ! {} lemma(s): {}{more}", lemmas.len(), clean(&shown.join(", ")));
        for (sfx, tag) in paradigm {
            let _ = writeln!(out, "  {sfx} {} # ;", clean(tag));
        }
    }
    out
}

/// Letters without accents, one char per char.
fn fold(s: &str) -> String {
    s.chars().map(base_letter).collect()
}

/// Longest prefix (folded) shared by all forms.
fn common_prefix(cells: &BTreeSet<Cell>) -> String {
    let mut forms = cells.iter().map(|(f, _)| fold(f));
    let Some(first) = forms.next() else { return String::new() };
    forms.fold(first, |p, f| p.chars().zip(f.chars()).take_while(|(a, b)| a == b).map(|(a, _)| a).collect())
}

/// Suffixes that at least two lemmas show in the same (POS, cell) when
/// split at their common prefix.
fn seed_suffixes(t: &Tables) -> BTreeSet<(&str, &str, &str)> {
    let mut count: BTreeMap<(&str, &str, &str), usize> = BTreeMap::new();
    for ((pos, _), cells) in &t.groups {
        let lcp = common_prefix(cells);
        for (form, rest) in cells {
            if let Some((_, sfx)) = split_at_stem(form, &lcp).filter(|(_, s)| !s.is_empty() && !rest.is_empty()) {
                *count.entry((pos, rest, sfx)).or_default() += 1;
            }
        }
    }
    count.into_iter().filter(|&(_, n)| n >= 2).map(|(k, _)| k).collect()
}

/// The prefix of the (folded) lemma after which the most forms end in a
/// seed suffix or nothing (ties: the longer prefix). Without any hit, the
/// common prefix of the forms.
fn choose_stem(lemma: &str, pos: &str, cells: &BTreeSet<Cell>, seed: &BTreeSet<(&str, &str, &str)>) -> String {
    let mut best = (0, String::new());
    for (i, c) in lemma.char_indices() {
        let p = &lemma[..i + c.len_utf8()];
        let hits = cells
            .iter()
            .filter(|(f, rest)| split_at_stem(f, p).is_some_and(|(_, s)| s.is_empty() || seed.contains(&(pos, rest.as_str(), s))))
            .count();
        if hits > 0 && hits >= best.0 {
            best = (hits, p.to_string());
        }
    }
    if best.0 == 0 {
        common_prefix(cells)
    } else {
        best.1
    }
}

/// `form` as (stem variant, suffix) when it starts with the folded `stem`.
fn split_at_stem<'f>(form: &'f str, stem: &str) -> Option<(&'f str, &'f str)> {
    if stem.is_empty() || !fold(form).starts_with(stem) {
        return None;
    }
    let at = form.char_indices().nth(stem.chars().count()).map_or(form.len(), |(i, _)| i);
    Some(form.split_at(at)).filter(writable)
}

/// A bare `0` means "empty" in a spec, so neither part may be `0`.
fn writable(&(stem, sfx): &(&str, &str)) -> bool {
    stem != "0" && sfx != "0"
}

/// `form` split before the longest suffix seen in its cell (ties: the more
/// frequent), leaving a non-empty stem variant.
fn split_before_known<'f>(form: &'f str, known: Option<&BTreeMap<&str, usize>>) -> Option<(&'f str, &'f str)> {
    let (sfx, _) = known?
        .iter()
        .filter(|(s, _)| s.len() < form.len() && form.ends_with(**s) && writable(&form.split_at(form.len() - s.len())))
        .max_by(|a, b| (a.0.len(), a.1).cmp(&(b.0.len(), b.1)).then_with(|| b.0.cmp(a.0)))?;
    Some(form.split_at(form.len() - sfx.len()))
}

/// `surface` or `surface:lemma`, with spaces in the lemma made `_`.
fn entry_head(surface: &str, lemma: &str) -> String {
    let lemma = clean(lemma);
    if lemma == surface {
        lemma
    } else {
        format!("{surface}:{lemma}")
    }
}

/// A token safe for a `.morph` line: no whitespace, no comment marker.
fn clean(s: &str) -> String {
    s.chars().map(|c| if c.is_whitespace() || c == '!' { '_' } else { c }).collect()
}

/// A lexicon-name fragment.
fn ident(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect()
}
