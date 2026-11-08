//! Part of speech and lemma against a UD treebank, and `PREFER` rules
//! learned from one.
//!
//! ```sh
//! cargo run --release --example treebank -- score <test.conllu> [SOURCE]
//! cargo run --release --example treebank -- learn <train.conllu> <dev.conllu> [SOURCE] [--fresh]
//! ```
//!
//! SOURCE is `--spec FILE` or `--unimorph TSV` (default: bundled English).
//! UniMorph tables list only words that inflect; `--closed TRAIN.conllu`
//! adds the function words (determiners, pronouns, adpositions,
//! conjunctions, adverbs) seen at least twice in a training treebank, most
//! frequent reading first. `--rules FILE` adds `PREFER` lines (from `learn`)
//! after the grammar's own.
//! `score` reads each sentence with `analyze_text` and prints the share of
//! words whose best reading has the treebank's lemma and part of speech
//! (nouns, verbs, adjectives, adverbs, determiners, pronouns, adpositions,
//! conjunctions, numbers).
//!
//! `learn` writes `PREFER` lines to stdout, best first, for the grammar to
//! append after its own (`--fresh` ignores them). Each round adds the rule
//! that fixes the most training words net of those it breaks, and learning
//! stops when that rule no longer helps the dev set. This is build-time
//! learning: the rules are plain spec text, ordered, never weighted, and
//! the lexer itself learns nothing. UD treebanks are CC BY-SA: fetch them,
//! never commit them.

use morph_fst::spec::{ContextRule, Sel};
use morph_fst::{Kind, Lexer, Part, Word};
use std::collections::HashMap;

struct Reading {
    class: String,
    lemma: String,
}

struct Tok {
    text: String,
    readings: Vec<Reading>,
    gold: Option<(&'static str, String)>,
}

type Sentence = Vec<Tok>;
/// (form, lemma, tag) of a function word.
type Closed = (String, String, &'static str);
/// (gold, read) → (misses, misses per word).
type Confusions<'a> = HashMap<(&'a str, &'a str), (usize, HashMap<&'a str, usize>)>;

fn derivational(tag: &str, listed: &[String]) -> bool {
    listed.iter().any(|d| tag == d || tag.strip_prefix(d.as_str()).is_some_and(|r| r.starts_with(':')))
}

fn span(parts: &[Part], listed: &[String]) -> (usize, Option<usize>) {
    let stem = parts.iter().position(|p| !derivational(p.tag(), listed)).unwrap_or(0);
    let last = parts.iter().enumerate().skip(stem + 1).filter(|(_, p)| derivational(p.tag(), listed)).map(|(i, _)| i).next_back();
    (stem, last)
}

/// As `Word` reads it: last derivation after the stem, else the stem.
fn class(parts: &[Part], listed: &[String]) -> String {
    let (stem, last) = span(parts, listed);
    parts.get(last.unwrap_or(stem)).map_or("", |p| p.tag()).split(['+', ';', ' ']).next().unwrap_or("").to_string()
}

fn whole_lemma(parts: &[Part], listed: &[String]) -> String {
    match span(parts, listed) {
        (_, Some(end)) => parts.iter().take(end + 1).map(Part::text).collect(),
        (stem, None) => parts.iter().take(stem).map(Part::text).chain(parts.get(stem).map(Part::lemma)).collect(),
    }
}

/// The treebank's coarse class of a grammar class.
fn coarse(class: &str) -> &'static str {
    match class.split([':', '.']).next().unwrap_or("") {
        "N" | "NMLZ" | "AGENT" | "NAME" | "PROPN" => "N",
        "V" | "VBZ" | "MOD" | "AUX" => "V",
        "ADJ" | "ADJZ" | "ABLE" => "ADJ",
        "ADV" => "ADV",
        "DET" | "ART" | "DEM" => "DET",
        "PRON" | "PRO" => "PRON",
        "PREP" | "ADP" | "POSTP" => "ADP",
        "CONJ" | "CCONJ" | "SCONJ" => "CONJ",
        "NUM" => "NUM",
        _ => "",
    }
}

fn gold(upos: &str) -> Option<&'static str> {
    Some(match upos {
        "NOUN" | "PROPN" => "N",
        "VERB" | "AUX" => "V",
        "ADJ" => "ADJ",
        "ADV" => "ADV",
        "DET" => "DET",
        "PRON" => "PRON",
        "ADP" => "ADP",
        "CCONJ" | "SCONJ" => "CONJ",
        "NUM" => "NUM",
        _ => return None,
    })
}

fn tok(w: &Word, listed: &[String]) -> Tok {
    let lemma = |r: &[Part]| if matches!(w.kind(), Kind::Known | Kind::Guessed) { whole_lemma(r, listed) } else { w.text().to_string() };
    let readings = std::iter::once(w.parts()).chain(w.alternatives().iter().map(Vec::as_slice)).map(|r| Reading { class: class(r, listed), lemma: lemma(r) }).collect();
    Tok { text: w.text().to_string(), readings, gold: None }
}

/// Every sentence of a CoNLL-U file as tokens, gold attached to the words
/// it lines up with.
fn load(lx: &Lexer, path: &str) -> Result<Vec<Sentence>, Box<dyn std::error::Error>> {
    let listed = &lx.mt.derivation;
    let data = std::fs::read_to_string(path)?;
    let mut out = Vec::new();
    for sent in data.split("\n\n") {
        let Some(text) = sent.lines().find_map(|l| l.strip_prefix("# text = ")) else { continue };
        let words = lx.analyze_text(text);
        let mut toks: Vec<Tok> = words.iter().map(|w| tok(w, listed)).collect();
        let mut at = 0;
        for l in sent.lines().filter(|l| !l.starts_with('#')) {
            let c: Vec<&str> = l.split('\t').collect();
            if c.len() < 4 || c[0].contains(['-', '.']) {
                continue;
            }
            let Some(start) = text.get(at..).and_then(|t| t.find(c[1])).map(|i| i + at) else { continue };
            at = start + c[1].len();
            let (Some(g), true) = (gold(c[3]), c[1].chars().all(char::is_alphabetic)) else { continue };
            if let Some(i) = words.iter().position(|w| w.span() == (start..at) && w.kind() != Kind::Literal) {
                if let Some(t) = toks.get_mut(i) {
                    t.gold = Some((g, c[2].to_lowercase()));
                }
            }
        }
        out.push(toks);
    }
    Ok(out)
}

fn matches(sels: &[Sel], r: &Reading, text: &str) -> bool {
    sels.iter().any(|s| match s {
        Sel::Tag(t) => r.class == *t || r.class.strip_prefix(t.as_str()).is_some_and(|x| x.starts_with(':')),
        Sel::Lemma(l) => r.lemma == *l,
        Sel::Form(f) => text == f,
    })
}

/// `Word`'s context pass on readings: (chosen reading, claimed by a rule).
fn simulate(s: &[Tok], rules: &[ContextRule]) -> Vec<(usize, bool)> {
    let mut out: Vec<(usize, bool)> = Vec::with_capacity(s.len());
    for (i, t) in s.iter().enumerate() {
        let mut state = (0, false);
        if t.readings.len() > 1 {
            for rule in rules {
                let near = if rule.before {
                    s.get(i + 1).and_then(|n| Some((n.readings.first()?, &n.text)))
                } else {
                    i.checked_sub(1).and_then(|j| Some((s[j].readings.get(out.get(j)?.0)?, &s[j].text)))
                };
                if !near.is_some_and(|(r, text)| matches(&rule.near, r, text)) {
                    continue;
                }
                if matches(&rule.prefer, &t.readings[0], &t.text) {
                    state.1 = true;
                    break;
                }
                if !rule.over.is_empty() && !matches(&rule.over, &t.readings[0], &t.text) {
                    continue;
                }
                state = (t.readings.iter().skip(1).position(|r| matches(&rule.prefer, r, &t.text)).map_or(0, |k| k + 1), true);
                break;
            }
        }
        out.push(state);
    }
    out
}

fn pos_right(t: &Tok, k: usize) -> bool {
    t.gold.as_ref().is_some_and(|(g, _)| t.readings.get(k).is_some_and(|r| coarse(&r.class) == *g))
}

/// (scored words, part of speech right) under `rules`.
fn accuracy(sents: &[Sentence], rules: &[ContextRule]) -> (usize, usize) {
    let (mut n, mut ok) = (0, 0);
    for s in sents {
        for (t, (k, _)) in s.iter().zip(simulate(s, rules)) {
            n += t.gold.is_some() as usize;
            ok += pos_right(t, k) as usize;
        }
    }
    (n, ok)
}

fn source(args: &[String]) -> Result<Lexer, Box<dyn std::error::Error>> {
    let flag = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1));
    let spec = match (flag("--spec"), flag("--unimorph")) {
        (Some(s), _) => std::fs::read_to_string(s)?,
        (None, Some(t)) => morph_fst::generate::from_unimorph(&std::fs::read_to_string(t)?)?.spec,
        (None, None) => return Ok(Lexer::bundled("english")?),
    };
    let base = Lexer::compile(&spec)?;
    let spec = match flag("--closed") {
        Some(train) => with_closed(&spec, &closed_words(&base, train)?)?,
        None => spec,
    };
    let spec = match flag("--rules") {
        // After the spec's own rules, and never inside a section.
        Some(f) => {
            let mut lines: Vec<String> = spec.lines().map(str::to_string).collect();
            let at = lines.iter().rposition(|l| l.starts_with("PREFER")).map_or(0, |i| i + 1);
            lines.splice(at..at, std::fs::read_to_string(f)?.lines().map(str::to_string));
            lines.join("\n") + "\n"
        }
        None => spec,
    };
    Ok(Lexer::compile(&spec)?)
}

/// (form, lemma, tag) of the function words seen at least twice, the most
/// frequent reading of a form first.
fn closed_words(lx: &Lexer, train: &str) -> Result<Vec<Closed>, Box<dyn std::error::Error>> {
    let mut count: HashMap<Closed, usize> = HashMap::new();
    for l in std::fs::read_to_string(train)?.lines().filter(|l| !l.starts_with('#')) {
        let c: Vec<&str> = l.split('\t').collect();
        if c.len() < 4 || c[0].contains(['-', '.']) {
            continue;
        }
        let tag = match c[3] {
            "DET" => "DET",
            "PRON" => "PRON",
            "ADP" => "ADP",
            "CCONJ" => "CCONJ",
            "SCONJ" => "SCONJ",
            "ADV" => "ADV",
            _ => continue,
        };
        let form = morph_fst::normalize::normalize(c[1], &lx.mt.norm);
        let lemma = morph_fst::normalize::normalize(c[2], &lx.mt.norm);
        if [&form, &lemma].iter().all(|s| !s.is_empty() && s.chars().all(char::is_alphabetic)) {
            *count.entry((form, lemma, tag)).or_default() += 1;
        }
    }
    let mut v: Vec<_> = count.into_iter().filter(|(_, n)| *n >= 2).collect();
    v.sort_by(|a, b| a.0 .0.cmp(&b.0 .0).then(b.1.cmp(&a.1)).then(a.0.cmp(&b.0)));
    Ok(v.into_iter().map(|(k, _)| k).collect())
}

/// `spec` with a `Closed` lexicon of `words` reachable from its root.
fn with_closed(spec: &str, words: &[Closed]) -> Result<String, Box<dyn std::error::Error>> {
    const ROOT: &str = "LEXICON Root\n";
    let at = spec.find(ROOT).ok_or("the spec has no `LEXICON Root`")? + ROOT.len();
    let mut lex = String::from("\nLEXICON TreebankClosed\n");
    for (form, lemma, tag) in words {
        lex.push_str(&format!("  {form}:{lemma} {tag} # ;\n"));
    }
    Ok(format!("{}  0 TreebankClosed ;\n{}{lex}", &spec[..at], &spec[at..]))
}

fn pc(a: usize, n: usize) -> f64 {
    100.0 * a as f64 / n.max(1) as f64
}

fn score(path: &str, args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut lx = source(args)?;
    let sents = load(&lx, path)?;
    let (mut n, mut pos, mut lem) = (0, 0, 0);
    let mut confused: Confusions = HashMap::new();
    for t in sents.iter().flatten() {
        if let Some((g, l)) = &t.gold {
            n += 1;
            pos += pos_right(t, 0) as usize;
            lem += (t.readings.first().is_some_and(|r| r.lemma == *l)) as usize;
            if !pos_right(t, 0) {
                let got = t.readings.first().map_or("", |r| coarse(&r.class));
                let e = confused.entry((g, got)).or_default();
                e.0 += 1;
                *e.1.entry(t.text.as_str()).or_default() += 1;
            }
        }
    }
    println!("{n} words: part of speech {:.1}%, lemma {:.1}%", pc(pos, n), pc(lem, n));
    if args.iter().any(|a| a == "--misses") {
        let mut v: Vec<_> = confused.into_iter().collect();
        v.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(&b.0)));
        for ((g, got), (k, words)) in v.into_iter().take(15) {
            let mut w: Vec<_> = words.into_iter().collect();
            w.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            let top: Vec<String> = w.iter().take(10).map(|(t, c)| format!("{t}:{c}")).collect();
            println!("  gold {g:4} read {got:4} {k:5}  {}", top.join(" "));
        }
    }
    let rules = std::mem::take(&mut lx.mt.context);
    let bare = load(&lx, path)?;
    let (n0, ok0) = accuracy(&bare, &[]);
    let (n1, ok1) = accuracy(&bare, &rules);
    println!("without context rules {:.1}%; {} rules, simulated {:.1}%", pc(ok0, n0), rules.len(), pc(ok1, n1));
    Ok(())
}

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Near {
    Tag(String),
    Lemma(String),
    Form(String),
}

type Cand = (String, Option<String>, bool, Near);

/// Spec-safe text for a quoted selector.
fn plain(s: &str) -> bool {
    !s.is_empty() && !s.contains(['"', '\'', ',', '!', ';']) && !s.contains(char::is_whitespace)
}

fn tags_of(class: &str) -> Vec<String> {
    let mut v = vec![class.to_string()];
    if let Some((base, _)) = class.split_once(':') {
        v.push(base.to_string());
    }
    v.retain(|t| plain(t));
    v
}

fn rule(c: &Cand) -> ContextRule {
    let near = match &c.3 {
        Near::Tag(t) => Sel::Tag(t.clone()),
        Near::Lemma(l) => Sel::Lemma(l.clone()),
        Near::Form(f) => Sel::Form(f.clone()),
    };
    ContextRule { prefer: vec![Sel::Tag(c.0.clone())], over: c.1.iter().map(|o| Sel::Tag(o.clone())).collect(), before: c.2, near: vec![near] }
}

fn line(c: &Cand) -> String {
    let near = match &c.3 {
        Near::Tag(t) => t.clone(),
        Near::Lemma(l) => format!("\"{l}\""),
        Near::Form(f) => format!("'{f}'"),
    };
    let over = c.1.as_ref().map(|o| format!(" OVER {o}")).unwrap_or_default();
    format!("PREFER {}{over} {} {near}", c.0, if c.2 { "BEFORE" } else { "AFTER" })
}

/// Net training words each candidate rule would fix, appended last.
fn gains(sents: &[Sentence], rules: &[ContextRule]) -> HashMap<Cand, i64> {
    let mut g: HashMap<Cand, i64> = HashMap::new();
    for s in sents {
        let state = simulate(s, rules);
        for (i, t) in s.iter().enumerate() {
            if t.readings.len() < 2 || t.gold.is_none() || state[i].1 {
                continue;
            }
            let r0 = &t.readings[0];
            let ok0 = pos_right(t, 0) as i64;
            let left = i.checked_sub(1).and_then(|j| Some((s[j].readings.get(state[j].0)?, &s[j].text)));
            let right = s.get(i + 1).and_then(|n| Some((n.readings.first()?, &n.text)));
            let mut overs: Vec<Option<String>> = vec![None];
            overs.extend(tags_of(&r0.class).into_iter().map(Some));
            let mut prefs: Vec<String> = t.readings.iter().skip(1).flat_map(|r| tags_of(&r.class)).collect();
            prefs.sort();
            prefs.dedup();
            for p in prefs {
                let sel = [Sel::Tag(p.clone())];
                if matches(&sel, r0, &t.text) {
                    continue;
                }
                let Some(k) = t.readings.iter().skip(1).position(|r| matches(&sel, r, &t.text)) else { continue };
                let d = pos_right(t, k + 1) as i64 - ok0;
                if d == 0 {
                    continue;
                }
                for (before, near) in [(false, left), (true, right)] {
                    let Some((nr, ntext)) = near else { continue };
                    let mut nears: Vec<Near> = tags_of(&nr.class).into_iter().map(Near::Tag).collect();
                    if plain(&nr.lemma) {
                        nears.push(Near::Lemma(nr.lemma.clone()));
                    }
                    if plain(ntext) && *ntext != nr.lemma {
                        nears.push(Near::Form(ntext.clone()));
                    }
                    for n in nears {
                        for o in &overs {
                            *g.entry((p.clone(), o.clone(), before, n.clone())).or_default() += d;
                        }
                    }
                }
            }
        }
    }
    g
}

fn learn(train: &str, dev: &str, args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut lx = source(args)?;
    let mut rules = std::mem::take(&mut lx.mt.context);
    if args.iter().any(|a| a == "--fresh") {
        rules.clear();
    }
    let (train, dev) = (load(&lx, train)?, load(&lx, dev)?);
    let (n, mut dev_ok) = accuracy(&dev, &rules);
    eprintln!("start: {} rules, dev {:.2}%", rules.len(), pc(dev_ok, n));
    loop {
        let g = gains(&train, &rules);
        let Some((best, gain)) = g.into_iter().max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0))) else { break };
        if gain <= 0 {
            break;
        }
        let mut next = rules.clone();
        next.push(rule(&best));
        let (_, ok) = accuracy(&dev, &next);
        if ok <= dev_ok {
            eprintln!("stop: `{}` fixes {gain} training words but not dev", line(&best));
            break;
        }
        dev_ok = ok;
        rules = next;
        println!("{}", line(&best));
        eprintln!("+{gain} train, dev {:.2}%", pc(dev_ok, n));
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: treebank score <test.conllu> [SOURCE] | learn <train.conllu> <dev.conllu> [SOURCE] [--fresh]";
    match args.first().map(String::as_str) {
        Some("score") => score(args.get(1).ok_or(usage)?, &args),
        Some("learn") => learn(args.get(1).ok_or(usage)?, args.get(2).ok_or(usage)?, &args),
        _ => Err(usage.into()),
    }
}
