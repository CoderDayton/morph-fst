//! Hybrid mode with stem shapes: an unknown stem takes its spelling rules,
//! inflection class and lemma from known stems that end the same way.

use morph_fst::{Lexer, Mode, Segment, WordOutput};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SPEC: &str = "
LEXICON Root
  0 Nouns ;
  0 Verbs ;
LEXICON Nouns @open=N
  city N Num ;
  party N Num ;
  cat N Num ;
  hat N Num ;
  book N Num ;
  cook N Num ;
  box N Num ;
  fox N Num ;
  king N Num ;
LEXICON Verbs @open=V
  walk V VInfl ;
  look V VInfl ;
  hook V VInfl ;
  bake V VInfl ;
  make V VInfl ;
  stop V VInfl @double ;
  drop V VInfl @double ;
LEXICON Num
  s PL # ;
  0 SG # ;
LEXICON VInfl
  s 3SG # ;
  ed PST # ;
  ing PROG # ;
  0 NFIN # ;
RULES
  y_ie: y -> ie / C _ +{s} ;
  e_del: e -> 0 / C _ +{ed,ing} ;
  double: . -> $$ / _ +{ed,ing} @double ;
  x_es: x -> xe / _ +{s} ;
";

/// `lemma?GUESS+TAG+TAG` per reading, best first.
fn readings(out: &WordOutput) -> Vec<String> {
    let WordOutput::Hybrid { all, .. } = out else { return Vec::new() };
    all.iter()
        .map(|segs| {
            segs.iter()
                .map(|s| match s {
                    Segment::Morph(m) => m.tag.clone(),
                    Segment::Unknown { lemma, guess, .. } => format!("{lemma}?{guess}"),
                })
                .collect::<Vec<_>>()
                .join("+")
        })
        .collect()
}

fn best(lx: &Lexer, w: &str) -> String {
    readings(&lx.lex(w, Mode::Hybrid)).into_iter().next().unwrap_or_default()
}

#[test]
fn spelling_rules_apply_to_unknown_stems() -> TestResult {
    let lx = Lexer::compile(SPEC)?;
    assert_eq!(best(&lx, "blorties"), "blorty?N+PL");
    assert_eq!(best(&lx, "glaking"), "glake?V+PROG");
    assert_eq!(best(&lx, "blopped"), "blop?V+PST");
    assert_eq!(best(&lx, "zaxes"), "zax?N+PL");
    Ok(())
}

#[test]
fn bare_unknown_words_get_zero_morphemes() -> TestResult {
    let lx = Lexer::compile(SPEC)?;
    assert_eq!(best(&lx, "blat"), "blat?N+SG");
    let out = lx.lex("blat", Mode::Hybrid);
    let WordOutput::Hybrid { segments, unknown_chars, .. } = &out else { return Err("not hybrid".into()) };
    assert_eq!(*unknown_chars, 2, "only `bl` is unexplained");
    let Some(Segment::Unknown { surface, pieces, .. }) = segments.first() else { return Err("no stem".into()) };
    assert_eq!(surface, "blat");
    assert!(!pieces.is_empty());
    Ok(())
}

#[test]
fn one_example_is_not_a_pattern() -> TestResult {
    let lx = Lexer::compile(SPEC)?;
    // Only `king` ends in -ing, so `blorfing` is not a noun like it.
    assert_eq!(best(&lx, "blorfing"), "blorf?V+PROG");
    Ok(())
}

#[test]
fn the_longest_shared_ending_decides() -> TestResult {
    // -ion is mostly verbal here, but only nouns end in -ation.
    let spec = "
LEXICON Root
  0 Nouns ;
  0 Verbs ;
LEXICON Nouns @open=N
  nation N Num ;
  station N Num ;
LEXICON Verbs @open=V
  champion V VInfl ;
  onion V VInfl ;
  union V VInfl ;
LEXICON Num
  0 SG # ;
LEXICON VInfl
  0 NFIN # ;
";
    let lx = Lexer::compile(spec)?;
    assert_eq!(best(&lx, "blation"), "blation?N+SG");
    Ok(())
}

#[test]
fn irregular_stems_are_not_patterns() -> TestResult {
    // speak/spoke and break/broke are listed whole: unproductive, so `-ak`
    // must not make an unknown word a verb.
    let spec = "
LEXICON Root
  0 Nouns ;
  0 Verbs ;
  0 Irregular ;
LEXICON Irregular
  spoke:speak V+PAST # ;
  broke:break V+PAST # ;
LEXICON Nouns @open=N
  book N Num ;
  cook N Num ;
  hook N Num ;
LEXICON Verbs @open=V
  speak V VInfl ;
  break V VInfl ;
LEXICON Num
  0 SG # ;
LEXICON VInfl
  0 NFIN # ;
";
    let lx = Lexer::compile(spec)?;
    assert_eq!(best(&lx, "zak"), "zak?N+SG");
    Ok(())
}

#[test]
fn every_reading_is_returned_ranked() -> TestResult {
    let lx = Lexer::compile(SPEC)?;
    let r = readings(&lx.lex("blooks", Mode::Hybrid));
    assert!(r.contains(&"blook?N+PL".to_string()), "{r:?}");
    assert!(r.contains(&"blook?V+3SG".to_string()), "{r:?}");
    let mut seen = r.clone();
    seen.dedup();
    assert_eq!(seen.len(), r.len(), "duplicate readings: {r:?}");
    Ok(())
}

#[test]
fn known_words_are_unchanged() -> TestResult {
    let lx = Lexer::compile(SPEC)?;
    assert_eq!(lx.lex("cities", Mode::Hybrid), lx.lex("cities", Mode::Strict));
    assert_eq!(lx.lex("stopped", Mode::Hybrid).tokens(), ["stop<V>", "ed<PST>"]);
    Ok(())
}

#[test]
fn shapes_survive_save_and_load() -> TestResult {
    let lx = Lexer::compile(SPEC)?;
    let back = Lexer::from_bytes(&lx.to_bytes())?;
    assert_eq!(back, lx);
    assert!(!back.mt.shapes.is_empty());
    assert_eq!(best(&back, "blorties"), "blorty?N+PL");
    Ok(())
}
