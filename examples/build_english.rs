//! Build `data/english.morph` from the hand-written grammar in
//! `tools/english/grammar.morph` and the open-class words of ESDB
//! (<https://github.com/en-wl/wordlist>, MIT-like license).
//!
//! ```sh
//! git clone --depth 1 https://github.com/en-wl/wordlist
//! cargo run --release --example build_english -- wordlist/data/scowl-pre.txt > data/english.morph
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::process::ExitCode;

const GRAMMAR: &str = include_str!("../tools/english/grammar.morph");

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Pos {
    N,
    V,
    Adj,
    Adv,
}

impl Pos {
    fn from_esdb(s: &str) -> Option<Pos> {
        Some(match s {
            "n" => Pos::N,
            "v" => Pos::V,
            "aj" => Pos::Adj,
            "av" => Pos::Adv,
            _ => return None,
        })
    }

    fn tag(self) -> &'static str {
        match self {
            Pos::N => "N",
            Pos::V => "V",
            Pos::Adj => "ADJ",
            Pos::Adv => "ADV",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Slot {
    Pl,
    Past,
    Ptcp,
    Prog,
    S3,
    Cmp,
    Sup,
}

impl Slot {
    fn tag(self) -> &'static str {
        match self {
            Slot::Pl => "PL",
            Slot::Past => "PAST",
            Slot::Ptcp => "PTCP",
            Slot::Prog => "PROG",
            Slot::S3 => "3SG",
            Slot::Cmp => "CMP",
            Slot::Sup => "SUP",
        }
    }
}

/// One sense of an ESDB lemma with its attested inflections.
#[derive(Debug)]
struct Word {
    lemma: String,
    pos: Pos,
    forms: BTreeMap<Slot, String>,
}

/// One line of `scowl-pre.txt`: `SIZE ...: [VARIANTS: ]LEMMA <POS[/CLASS]> [{SENSE}] [(USAGE)][: FORMS]`.
struct Line<'a> {
    size: u32,
    keep: bool,
    lemma: &'a str,
    pos: &'a str,
    sense: &'a str,
    forms: &'a str,
}

/// American spelling at variant level 0 or 1 (`A`, `_`, `A.`, `_.`), or no variant info.
fn american(info: &str) -> bool {
    info.trim().is_empty() || info.split_whitespace().any(|t| matches!(t, "A" | "_" | "A." | "_."))
}

fn parse_line(line: &str) -> Option<Line<'_>> {
    let line = line.split(" #").next()?;
    let (info, rest) = line.split_once(": ")?;
    let size = info.split_whitespace().next()?.parse().ok()?;
    let foreign = info.split_whitespace().any(|t| matches!(t, "GB" | "CA" | "AU"));
    let lt = rest.find('<')?;
    let (variants, lemma) = match rest[..lt].rfind(": ") {
        Some(i) => (&rest[..i], rest[i + 2..lt].trim()),
        None => ("", rest[..lt].trim()),
    };
    let gt = lt + rest[lt..].find('>')?;
    let pos = &rest[lt + 1..gt];
    let (mut r, mut sense, mut usage) = (rest[gt + 1..].trim_start(), "", "");
    loop {
        if let Some(s) = r.strip_prefix('{') {
            let end = s.find('}')?;
            (sense, r) = (&s[..end], s[end + 1..].trim_start());
        } else if let Some(s) = r.strip_prefix('(') {
            let end = s.find(')')?;
            (usage, r) = (&s[..end], s[end + 1..].trim_start());
        } else {
            break;
        }
    }
    let forms = r.strip_prefix(':').map_or("", str::trim);
    let keep = !foreign && american(variants) && !usage.contains("offensive") && !usage.contains("vulgar");
    Some(Line { size, keep, lemma, pos, sense, forms })
}

/// Split `a, (b | ~: c), -` at top-level commas.
fn split_forms(s: &str) -> Vec<&str> {
    let (mut out, mut depth, mut start) = (Vec::new(), 0i32, 0);
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    if !s[start..].trim().is_empty() {
        out.push(s[start..].trim());
    }
    out
}

/// The preferred American spelling of one form slot, dropping archaic,
/// near-nonexistent and inapplicable forms.
fn pick_form(slot: &str) -> Option<String> {
    let alts: Vec<&str> = match slot.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        Some(inner) => inner.split(" | ").collect(),
        None => vec![slot],
    };
    let word = alts.into_iter().find_map(|a| match a.rsplit_once(": ") {
        Some((info, w)) => american(info).then_some(w),
        None => Some(a),
    })?;
    let word = word.trim();
    let bare = word.trim_end_matches(['*', '-', '†']);
    if bare.is_empty() || bare == "-" || bare.ends_with(['~', '!', '@']) {
        return None;
    }
    Some(bare.to_string())
}

/// Parse ESDB open-class lemmas of size `max` or less (lower-case, American).
fn parse(esdb: &str, max: u32) -> Vec<Word> {
    let mut words: Vec<Word> = Vec::new();
    let mut index: BTreeMap<(String, Pos, String), usize> = BTreeMap::new();
    let mut last: Option<usize> = None;
    let mut skip_lemmas: BTreeSet<String> = BTreeSet::new();
    for raw in esdb.lines() {
        if raw.trim().is_empty() {
            last = None;
            continue;
        }
        let Some(l) = parse_line(raw) else { continue };
        let (base, class) = l.pos.split_once('/').unwrap_or((l.pos, ""));
        let pos = Pos::from_esdb(base).filter(|_| class.is_empty());
        let usable = l.keep && l.size <= max;
        let id = if l.lemma == "-" {
            last.filter(|_| usable)
        } else {
            last = None;
            let ok = usable && l.lemma.len() > 1 && l.lemma.bytes().all(|b| b.is_ascii_lowercase());
            let Some(pos) = pos.filter(|_| ok) else { continue };
            let key = (l.lemma.to_string(), pos, l.sense.to_string());
            let id = *index.entry(key).or_insert_with(|| {
                words.push(Word { lemma: l.lemma.to_string(), pos, forms: BTreeMap::new() });
                words.len() - 1
            });
            last = Some(id);
            Some(id)
        };
        let Some(w) = id.and_then(|i| words.get_mut(i)) else { continue };
        let forms: Vec<Option<String>> = split_forms(l.forms).into_iter().map(pick_form).collect();
        let slots: &[Slot] = match (w.pos, forms.len()) {
            (_, 0) => &[],
            (Pos::N, _) => &[Slot::Pl],
            (Pos::V, 3) => &[Slot::Past, Slot::Prog, Slot::S3],
            (Pos::V, 4) => &[Slot::Past, Slot::Ptcp, Slot::Prog, Slot::S3],
            (Pos::V, _) => {
                skip_lemmas.insert(w.lemma.clone());
                &[]
            }
            (Pos::Adj | Pos::Adv, _) => &[Slot::Cmp, Slot::Sup],
        };
        let forms = forms.into_iter();
        let forms: Vec<Option<String>> = if w.pos == Pos::N {
            // n0 [ns] [np] [nsp]: the plural is the first form without an apostrophe
            forms.flatten().filter(|f| !f.contains('\'')).take(1).map(Some).collect()
        } else {
            forms.collect()
        };
        for (slot, form) in slots.iter().zip(forms) {
            if let Some(f) = form {
                w.forms.entry(*slot).or_insert(f);
            }
        }
    }
    for w in &mut words {
        if let Some(p) = w.forms.get(&Slot::Past).cloned() {
            w.forms.entry(Slot::Ptcp).or_insert(p);
        }
    }
    words.retain(|w| !skip_lemmas.contains(&w.lemma) || w.pos != Pos::V);
    words
}

const NOUN_FLAGS: [&str; 4] = ["", "@ves", "@oes", "@double"];
const VERB_CLASSES: [&str; 5] = ["VerbInfl", "VerbAble", "VerbMent", "VerbIon", "VerbAtion"];
const VERB_FLAGS: [&str; 3] = ["", "@double", "@ck"];
const ADJ_CLASSES: [&str; 3] = ["AdjInfl", "AdjEn", "AdjDerivInfl"];
const ADJ_FLAGS: [&str; 2] = ["", "@double"];

/// Every (continuation class, flag) a word of `pos` may take.
fn candidates(pos: Pos) -> Vec<(&'static str, &'static str)> {
    let cross = |cs: &[&'static str], fs: &[&'static str]| cs.iter().flat_map(|c| fs.iter().map(move |f| (*c, *f))).collect();
    match pos {
        Pos::N => cross(&["NounCont"], &NOUN_FLAGS),
        Pos::V => cross(&VERB_CLASSES, &VERB_FLAGS),
        Pos::Adj => cross(&ADJ_CLASSES, &ADJ_FLAGS),
        Pos::Adv => Vec::new(),
    }
}

/// Tag of the probe entry for candidate (`class`, `flag`), e.g. `V#4`.
fn probe_tag(pos: Pos, class: &str, flag: &str) -> String {
    let i = candidates(pos).iter().position(|&c| c == (class, flag)).unwrap_or(usize::MAX);
    format!("{}#{i}", pos.tag())
}

fn entry(lemma: &str, tag: &str, class: &str, flag: &str) -> String {
    let flag = if flag.is_empty() { String::new() } else { format!(" {flag}") };
    format!("  {lemma:<20} {tag:<6} {class}{flag} ;")
}

fn full_form(form: &str, w: &Word, slot: Slot) -> String {
    let surface = if form == w.lemma { form.to_string() } else { format!("{form}:{}", w.lemma) };
    format!("  {surface:<24} {:<8} # ;", format!("{}+{}", w.pos.tag(), slot.tag()))
}

/// Generated lexicon lines, keyed by (POS, lemma) so derived words can be dropped.
#[derive(Default)]
struct Lexicon {
    irregular: BTreeSet<String>,
    stems: BTreeMap<(Pos, String), BTreeSet<String>>,
}

impl Lexicon {
    fn render(&self, grammar: &str) -> morph_fst::Result<String> {
        let mut out = vec!["! ---------------------------------------------------------------------".to_string()];
        out.push("!  Generated from ESDB: irregular forms, adverbs, open-class stems".into());
        out.push(out[0].clone());
        out.push("LEXICON Irregular".into());
        out.extend(self.irregular.iter().cloned());
        // Equal analyses rank by entry order: adjectives, nouns, verbs measured best on UniMorph.
        for (pos, name) in [(Pos::Adv, "Adverbs"), (Pos::Adj, "AdjStems @open=ADJ"), (Pos::N, "NounStems @open=N"), (Pos::V, "VerbStems @open=V")] {
            out.push(String::new());
            out.push(format!("LEXICON {name}"));
            out.extend(self.stems.iter().filter(|((p, _), _)| *p == pos).flat_map(|(_, lines)| lines.iter().cloned()));
        }
        let generated = out.join("\n") + "\n";
        if !grammar.contains("!@GENERATED") {
            return Err(morph_fst::Error::Io("grammar has no !@GENERATED marker".into()));
        }
        Ok(grammar.replacen("!@GENERATED\n", &generated, 1))
    }
}

fn analyses(lx: &morph_fst::Lexer, w: &str) -> Vec<morph_fst::Analysis> {
    match lx.analyze(w, 4096) {
        Some(morph_fst::WordOutput::Analyzed { all, .. }) => all,
        _ => Vec::new(),
    }
}

/// What the probe lexer found: inflections of each attested form, and
/// (lemma, probe tag, suffix tag) for every word starting with a probe stem.
struct Evidence {
    parses: BTreeMap<String, Vec<morph_fst::Analysis>>,
    derived: BTreeSet<(String, String, String)>,
}

impl Evidence {
    fn collect(lx: &morph_fst::Lexer, words: &[Word]) -> Evidence {
        let mut parses = BTreeMap::new();
        for w in words {
            for s in std::iter::once(&w.lemma).chain(w.forms.values()) {
                if !parses.contains_key(s) {
                    parses.insert(s.clone(), analyses(lx, s));
                }
            }
        }
        let mut derived = BTreeSet::new();
        for a in parses.values().flatten() {
            if let (Some(stem), Some(sfx)) = (a.first(), a.get(1)) {
                if stem.tag.contains('#') {
                    derived.insert((stem.lemma.clone(), stem.tag.clone(), sfx.tag.clone()));
                }
            }
        }
        Evidence { parses, derived }
    }

    /// Does the probe stem `tag` inflect `w` into its attested `slot` form?
    /// `None` when ESDB lists no such form.
    fn inflects(&self, w: &Word, tag: &str, slot: Slot) -> Option<bool> {
        let form = w.forms.get(&slot)?;
        let all = self.parses.get(form)?;
        Some(all.iter().any(|a| {
            a.len() == 2
                && a.first().is_some_and(|m| m.lemma == w.lemma && m.tag == tag)
                && a.get(1).is_some_and(|m| m.tag == slot.tag())
        }))
    }

    fn derives(&self, w: &Word, tag: &str, suffix_tags: &[&str]) -> bool {
        suffix_tags.iter().any(|s| self.derived.contains(&(w.lemma.clone(), tag.to_string(), s.to_string())))
    }
}

fn fit_noun(w: &Word, ev: &Evidence, lex: &mut Lexicon) -> (&'static str, &'static str) {
    let Some(pl) = w.forms.get(&Slot::Pl) else { return ("NounCont", "") };
    match NOUN_FLAGS.into_iter().find(|f| ev.inflects(w, &probe_tag(Pos::N, "NounCont", f), Slot::Pl) == Some(true)) {
        Some(f) => ("NounCont", f),
        None => {
            lex.irregular.insert(full_form(pl, w, Slot::Pl));
            ("NounNoPl", "")
        }
    }
}

fn fit_verb(w: &Word, ev: &Evidence, lex: &mut Lexicon) -> (&'static str, &'static str) {
    let slots = [Slot::Past, Slot::Ptcp, Slot::Prog, Slot::S3];
    let score = |f: &str| slots.iter().filter(|&&s| ev.inflects(w, &probe_tag(Pos::V, "VerbInfl", f), s) == Some(true)).count();
    let mut flag = "";
    for f in VERB_FLAGS {
        if score(f) > score(flag) {
            flag = f;
        }
    }
    let tag = probe_tag(Pos::V, "VerbInfl", flag);
    let regular = |s: Slot| ev.inflects(w, &tag, s) != Some(false);
    let irregular = !regular(Slot::Past) || !regular(Slot::Ptcp);
    for s in slots {
        let listed = if matches!(s, Slot::Past | Slot::Ptcp) { irregular } else { !regular(s) };
        if let Some(f) = w.forms.get(&s).filter(|_| listed) {
            lex.irregular.insert(full_form(f, w, s));
        }
    }
    if irregular {
        return ("VerbIrr", flag);
    }
    let own: [(&str, &[&str]); 3] = [("VerbIon", &["NMLZ", "ADJZ:IVE"]), ("VerbAtion", &["NMLZ"]), ("VerbMent", &["NMLZ"])];
    let class = own
        .into_iter()
        .find(|(c, sfx)| ev.derives(w, &probe_tag(Pos::V, c, flag), sfx))
        .map(|(c, _)| c)
        .or_else(|| VERB_CLASSES.iter().any(|c| ev.derives(w, &probe_tag(Pos::V, c, flag), &["ABLE"])).then_some("VerbAble"))
        .unwrap_or("VerbInfl");
    (class, flag)
}

fn fit_adj(w: &Word, ev: &Evidence, lex: &mut Lexicon) -> (&'static str, &'static str) {
    let slots = [Slot::Cmp, Slot::Sup];
    if slots.iter().all(|s| !w.forms.contains_key(s)) {
        return ("AdjDerivInfl", "");
    }
    let regular = ADJ_FLAGS
        .into_iter()
        .find(|f| slots.iter().all(|&s| ev.inflects(w, &probe_tag(Pos::Adj, "AdjInfl", f), s) != Some(false)));
    match regular {
        Some(f) if ev.derives(w, &probe_tag(Pos::Adj, "AdjEn", f), &["VBZ"]) => ("AdjEn", f),
        Some(f) => ("AdjInfl", f),
        None => {
            for s in slots {
                if let Some(f) = w.forms.get(&s) {
                    lex.irregular.insert(full_form(f, w, s));
                }
            }
            ("AdjDerivInfl", "")
        }
    }
}

/// The POS a suffix derives when the grammar's derivation is transparent
/// (`happi|ness`); opaque-prone ones (`-ion`, `-al`, `-er`, `-ing`) are left out.
fn transparent(m: &morph_fst::Morph) -> Option<Pos> {
    Some(match (m.lemma.as_str(), m.tag.as_str()) {
        ("ly", "ADV") => Pos::Adv,
        ("ness" | "ity" | "ment" | "ship" | "hood" | "ism" | "ist", _) => Pos::N,
        ("ful" | "less" | "ous" | "able" | "ish", _) => Pos::Adj,
        ("ize", _) => Pos::V,
        _ => return None,
    })
}

/// The POS of a word the grammar derives from another stem (`un|happy`,
/// `un|tie`, `happi|ness`), if `a` is such a derivation.
fn derivation(a: &morph_fst::Analysis) -> Option<Pos> {
    let mut it = a.iter().peekable();
    let prefix = it.next_if(|m| m.tag.starts_with("PFX:")).map(|m| (m.lemma.as_str(), m.tag.as_str()));
    let mut pos = match (prefix, it.next()?.tag.as_str()) {
        (None | Some(("un" | "in" | "im" | "non", "PFX:NEG")), "N") => Pos::N,
        (None | Some(("un" | "in" | "im" | "non", "PFX:NEG")), "ADJ") => Pos::Adj,
        (None | Some(("un", "PFX:REV")), "V") => Pos::V,
        _ => return None,
    };
    let prefixed = prefix.is_some();
    let mut steps = usize::from(prefixed);
    for m in it {
        pos = transparent(m)?;
        steps += 1;
    }
    (steps > 0).then_some(pos)
}

/// Surfaces of the grammar's function words with tags in `tags`.
fn closed_words(grammar: &str, tags: &[&str]) -> BTreeSet<String> {
    let section = grammar.split("LEXICON Closed").nth(1).unwrap_or("");
    let section = section.split("LEXICON ").next().unwrap_or("").split("!@GENERATED").next().unwrap_or("");
    section
        .lines()
        .filter_map(|l| {
            let mut t = l.split_whitespace();
            let surface = t.next()?.split(':').next()?;
            let tag = t.next()?;
            (tags.is_empty() || tags.contains(&tag)).then(|| surface.to_string())
        })
        .collect()
}

const HEADER: &str = "\
! =====================================================================
!  english.morph: generated by examples/build_english.rs, do not edit.
!  The grammar is tools/english/grammar.morph; the lexicon comes from
!  ESDB (English Speller Database, previously SCOWL v2),
!  https://github.com/en-wl/wordlist, under this notice:
!
!  Copyright 2000-2026 by Kevin Atkinson
!
!  Permission to use, copy, modify, distribute, and sell any part of the
!  English Speller Database (ESDB, previously known as SCOWLv2), or word
!  lists created from it, is hereby granted without fee, provided that the
!  above copyright notice appears in all copies and that both the above
!  copyright notice and this notice appear in supporting documentation.
!  Kevin Atkinson makes no representations about the suitability of this
!  database for any purpose.  It is provided \"as is\" without express or
!  implied warranty.
! =====================================================================

";

/// The full spec: `grammar` with the lexicon fitted from ESDB text.
fn build(grammar: &str, esdb: &str, max_size: u32) -> morph_fst::Result<String> {
    let function = closed_words(grammar, &["DET", "PRON"]);
    let closed = closed_words(grammar, &[]);
    let mut words = parse(esdb, max_size);
    words.retain(|w| w.lemma != "be" && !function.contains(&w.lemma) && !(w.pos == Pos::Adv && closed.contains(&w.lemma)));

    let mut probe = Lexicon::default();
    for w in &words {
        let lines = probe.stems.entry((w.pos, w.lemma.clone())).or_default();
        for (class, flag) in candidates(w.pos) {
            lines.insert(entry(&w.lemma, &probe_tag(w.pos, class, flag), class, flag));
        }
    }
    let ev = Evidence::collect(&morph_fst::Lexer::compile(&probe.render(grammar)?)?, &words);

    let mut lex = Lexicon::default();
    for w in &words {
        let (class, flag) = match w.pos {
            Pos::N => fit_noun(w, &ev, &mut lex),
            Pos::V => fit_verb(w, &ev, &mut lex),
            Pos::Adj => fit_adj(w, &ev, &mut lex),
            Pos::Adv => ("#", ""),
        };
        lex.stems.entry((w.pos, w.lemma.clone())).or_default().insert(entry(&w.lemma, w.pos.tag(), class, flag));
    }

    let lx = morph_fst::Lexer::compile(&lex.render(grammar)?)?;
    let derived: Vec<(Pos, String)> = words
        .iter()
        .filter(|w| analyses(&lx, &w.lemma).iter().any(|a| derivation(a) == Some(w.pos)))
        .map(|w| (w.pos, w.lemma.clone()))
        .collect();
    for key in &derived {
        lex.stems.remove(key);
    }
    let count = |p: Pos| lex.stems.keys().filter(|(q, _)| *q == p).count();
    eprintln!(
        "{} nouns, {} verbs, {} adjectives, {} adverbs, {} irregular forms; {} derived words left to the grammar",
        count(Pos::N),
        count(Pos::V),
        count(Pos::Adj),
        count(Pos::Adv),
        lex.irregular.len(),
        derived.len()
    );
    Ok(HEADER.to_string() + &lex.render(grammar)?)
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: build_english <scowl-pre.txt> [max-size, default 60]");
        return ExitCode::FAILURE;
    };
    let max = args.next().and_then(|s| s.parse().ok()).unwrap_or(60);
    let run = || -> morph_fst::Result<String> {
        let esdb = std::fs::read_to_string(&path).map_err(|e| morph_fst::Error::Io(format!("{path}: {e}")))?;
        build(GRAMMAR, &esdb, max)
    };
    match run() {
        Ok(spec) => {
            print!("{spec}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ESDB: &str = "\
35 [12dicts]: stop <v>
35 [12dicts]: - <v>: stopped, stopping, stops

35 [12dicts]: go <v>: went, going, goes
35 [12dicts]: - <v>: -, gone, -, -

35 [12dicts]: panic <v>
35 [12dicts]: - <v>: panicked, panicking, panics

35 [12dicts]: visit <v>: visited, visiting, visits

35 [12dicts]: move <v>: moved, moving, moves

35 [12dicts]: movement <n>: movements, movement's

35 [12dicts]: knife <n>: knives
35 [12dicts]: - <n>: knife's

35 [12dicts]: roof <n>: roofs, roof's

35 [12dicts]: mouse <n> {animal}: mice
35 [12dicts]: - <n> {animal}: -, mouse's, mice's

35 [12dicts]: happy <aj>: happier, happiest

35 [12dicts]: happiness <n>
80 [12dicts]: - <n>: happinesses~

35 [12dicts]: good <aj>: better, best

35 [12dicts]: quick <aj>: quicker, quickest

35 [12dicts]: quickly <av>

35 [12dicts]: abroad <av>

35 [12dicts]: tie <v>: tied, tying, ties

50 [12dicts]: untie <v>: untied, untying, unties

85 [ukacd]: aasvogel <n>: aasvogels

60 [12dicts] 70 [3of6]: Aaron <n/person>: Aaron's

35 [12dicts]: B C D: colour <n>
35 [12dicts]: A Cv DV: color <n>: colors
";

    fn spec() -> String {
        build(GRAMMAR, ESDB, 60).unwrap_or_else(|e| panic!("{e}"))
    }

    fn has(spec: &str, line: &str) -> bool {
        let want: Vec<&str> = line.split_whitespace().collect();
        spec.lines().any(|l| l.split_whitespace().eq(want.iter().copied()))
    }

    #[test]
    fn each_word_gets_the_class_that_reproduces_its_inflections() {
        let s = spec();
        for line in [
            "stop V VerbInfl @double ;",
            "panic V VerbInfl @ck ;",
            "visit V VerbInfl ;",
            "knife N NounCont @ves ;",
            "roof N NounCont ;",
            "happy ADJ AdjInfl ;",
            "color N NounCont ;",
        ] {
            assert!(has(&s, line), "missing `{line}` in\n{s}");
        }
    }

    #[test]
    fn forms_no_class_reproduces_are_listed_whole() {
        let s = spec();
        for line in [
            "go V VerbIrr ;",
            "went:go V+PAST # ;",
            "gone:go V+PTCP # ;",
            "goes:go V+3SG # ;",
            "mouse N NounNoPl ;",
            "mice:mouse N+PL # ;",
            "better:good ADJ+CMP # ;",
            "best:good ADJ+SUP # ;",
        ] {
            assert!(has(&s, line), "missing `{line}` in\n{s}");
        }
    }

    #[test]
    fn attested_derivations_choose_the_derivational_class() {
        assert!(has(&spec(), "move V VerbMent ;"));
    }

    #[test]
    fn words_the_grammar_derives_are_not_listed() {
        let s = spec();
        assert!(has(&s, "abroad ADV # ;"));
        for w in ["happiness", "movement", "quickly", "untie"] {
            assert!(!s.lines().any(|l| l.split_whitespace().next() == Some(w)), "{w} listed");
        }
    }

    #[test]
    fn rare_proper_and_british_words_are_skipped() {
        let s = spec();
        for w in ["aasvogel", "aaron", "Aaron", "colour"] {
            assert!(!s.lines().any(|l| l.split_whitespace().next() == Some(w)), "{w} listed");
        }
    }

    #[test]
    fn the_spec_compiles_and_analyzes() {
        let lx = morph_fst::Lexer::compile(&spec()).unwrap_or_else(|e| panic!("{e}"));
        let Some(morph_fst::WordOutput::Analyzed { best, .. }) = lx.analyze("stopped", 64) else { panic!("stopped") };
        assert_eq!(best.iter().map(|m| format!("{}<{}>", m.lemma, m.tag)).collect::<Vec<_>>(), ["stop<V>", "ed<PAST>"]);
    }
}
