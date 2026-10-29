//! Beyond surface-true harmony, and exceptions without hand marking:
//! * `OPTION harmony underlying` (opacity; abstract `@tier=` forms),
//! * `INDUCE` + corpus: exceptions learned from raw text,
//! * `INDUCE` at runtime: unknown stems infer their own exception.

use morph_fst::{Lexer, Mode};
use morph_fst::induce::induce;
use morph_fst::oracle::{self, Gloss};
use morph_fst::rng::Rng;
use morph_fst::spec::{self, parse_spec};
use morph_fst::{languages, Segment, WordOutput};
use std::collections::BTreeSet;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Opacity: `ï` is a front vowel underlyingly, but a rule turns it into
/// `a` before the plural. Harmony (both the suffix, reading left, and the
/// prefix, reading right) follows the underlying `ï`. `pil` is abstractly
/// back (`@tier=pul`).
const OPAQUE: &str = r#"
OPTION harmony underlying
CLASS FRONT = e i ï
CLASS BACK = a o u
FEATURE h
  [FRONT] -> f
  [BACK] -> b
RFEATURE r
  [FRONT] -> f
  [BACK] -> b
ARCHI A
  h=f -> e
  * -> a
ARCHI P
  r=b -> o
  * -> i
LEXICON Root
  0 Pfx ;
LEXICON Pfx
  m{P}   P   Stems ;
  0 Stems ;
LEXICON Stems @open=S
  kït    S   Sfx ;
  kot    S   Sfx ;
  pil    S   Sfx @tier=pul ;
LEXICON Sfx
  l{A}r  PL  # ;
  0 # ;
RULES
  absn: ï -> a / _ [*]* +{l{A}r} ;
"#;

const CHAINS_INDUCE: &str = r#"
CLASS PLUS = i e
CLASS MINUS = a o
RFEATURE h
  [PLUS] -> p
  [MINUS] -> m
ARCHI A
  h=m -> a
  * -> e
INDUCE h IN Stems
LEXICON Root
  0 P1 ;
LEXICON P1
  b{A}  P1  Stems ;
  0 Stems ;
LEXICON Stems @open=S
  tik   S  # ;
  tok   S  # ;
"#;

/// Like the hand-pinned toy in right_harmony.rs, but with nothing marked.
const PINS_INDUCE: &str = r#"
OPTION induce_min_count 1
CLASS PLUS = i e
CLASS MINUS = a o
RFEATURE h
  [PLUS] -> p
  [MINUS] -> m
FEATURE lf
  [PLUS] -> p
  [MINUS] -> m
ARCHI A
  h=m -> a
  * -> e
ARCHI O
  lf=m -> o
  * -> i
INDUCE h lf IN Stems
LEXICON Root
  0 P1 ;
LEXICON P1
  b{A}  P1  P2 ;
  0 P2 ;
LEXICON P2
  d{A}  P2  Stems ;
  0 Stems ;
LEXICON Stems
  tik   S  Sfx ;
  tok   S  Sfx ;
  pil   S  Sfx ;
LEXICON Sfx
  n{O}  N  # ;
  0 # ;
"#;

fn lexer(text: &str) -> morph_fst::Result<Lexer> {
    Lexer::compile(text)
}

/// (root surface, learned flag) for every learned exception.
fn learned_flags(ind: &morph_fst::Induction) -> BTreeSet<(String, String)> {
    ind.exceptions().filter_map(|d| d.flag.clone().map(|f| (d.surface.clone(), f))).collect()
}

fn accepts(f: &Lexer, w: &str) -> bool {
    matches!(f.lex(w, Mode::Strict), WordOutput::Analyzed { .. })
}

#[test]
fn underlying_harmony_is_opaque() -> TestResult {
    let f = lexer(OPAQUE)?;
    for w in ["kït", "katler", "kot", "kotlar", "pil", "pillar", "mikït", "mikatler", "mokot", "mokotlar", "mopil", "mopillar"] {
        assert!(accepts(&f, w), "should accept {w}");
    }
    // surface-true harmony would pick these; underlying harmony rejects them
    for w in ["katlar", "kïtler", "kotler", "piller", "mokatler", "makatlar", "mipil", "mipiller", "mokït"] {
        assert!(!accepts(&f, w), "should reject {w}");
    }
    // @tier needs underlying mode
    let err = spec::load("LEXICON Root\n x N # @tier=y ;").err().ok_or("@tier without underlying mode compiled")?;
    assert!(err.to_string().contains("harmony underlying"), "{err}");
    Ok(())
}

#[test]
fn turkish_exceptions_are_learned_not_marked() -> TestResult {
    let unmarked = languages::TURKISH.replace(" @set:vow=fu", "") + "\nINDUCE vow IN Nouns Adjs\n";
    let parsed = parse_spec(&unmarked)?;
    assert!(parsed.lexicons.iter().flat_map(|l| &l.entries).all(|e| !e.flags.iter().any(|f| f.starts_with("set:"))));
    let corpus = include_str!("../data/turkish_corpus.txt");
    let ind = induce(&parse_spec(&unmarked)?, corpus)?;
    let learned = learned_flags(&ind);
    let want: BTreeSet<(String, String)> =
        ["saat", "harf", "hayal", "kalp"].iter().map(|s| (s.to_string(), "set:vow=fu".to_string())).collect();
    assert_eq!(learned, want);
    // the learned lexer behaves exactly like the hand-marked one (strict)
    let hand = Lexer::compile(languages::TURKISH)?;
    let auto = Lexer::from_grammar(&spec::compile_grammar(&ind.spec)?);
    assert_eq!(hand.mt.entries, auto.mt.entries);
    let words: Vec<String> = oracle::generate(&parse_spec(languages::TURKISH)?, 4, 50_000)?.surfaces.into_keys().collect();
    let mut rng = Rng::new(3);
    for _ in 0..20_000 {
        let mut w: Vec<char> = rng.pick(&words).chars().collect();
        if rng.chance(1, 3) {
            let k = rng.below(w.len());
            w[k] = *rng.pick(&['a', 'e', 'ı', 'i', 'u', 'ü', 'l', 'r']);
        }
        let w: String = w.into_iter().collect();
        assert_eq!(hand.lex(&w, Mode::Strict), auto.lex(&w, Mode::Strict), "{w}");
    }
    // the annotated spec reproduces the same grammar
    let annotated = ind.annotate(&unmarked);
    assert!(annotated.lines().any(|l| l.trim_start().starts_with("saat") && l.contains("@set:vow=fu ;  ! learned")));
    assert_eq!(Lexer::compile(&annotated)?.mt.entries, auto.mt.entries);
    // without a corpus nothing is learned and saatler is rejected again
    let bare = Lexer::compile(&unmarked)?;
    assert!(matches!(bare.lex("saatler", Mode::Strict), WordOutput::Fallback(_)));
    Ok(())
}

#[test]
fn right_and_left_exceptions_are_learned_per_feature() -> TestResult {
    let corpus = "bapil badapil pilno pilno tik betik tikni batok tokno";
    let ind = induce(&parse_spec(PINS_INDUCE)?, corpus)?;
    let learned = learned_flags(&ind);
    let want: BTreeSet<(String, String)> = [("pil", "rset:h=m"), ("pil", "set:lf=m")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    assert_eq!(learned, want);
    let (lx, _) = Lexer::compile_with_corpus(PINS_INDUCE, corpus)?;
    for w in ["bapil", "badapil", "bapilno", "pilno", "betik", "tikni"] {
        assert!(matches!(lx.lex(w, Mode::Strict), WordOutput::Analyzed { .. }), "{w}");
    }
    for w in ["bepil", "pilni", "bapilni", "batik"] {
        assert!(matches!(lx.lex(w, Mode::Strict), WordOutput::Fallback(_)), "{w}");
    }
    Ok(())
}

/// Readings that print the same text but guess different exceptions or
/// classes for the unknown stem are distinct readings.
#[test]
fn bare_head_readings_differ_by_guess() -> TestResult {
    let tr = Lexer::compile(&(languages::TURKISH.to_string() + "\nINDUCE vow IN Nouns Adjs\n"))?;
    let Some(guesses) = (match tr.lex("evlar", Mode::Hybrid) {
        WordOutput::Hybrid { all, .. } => Some(
            all.iter()
                .flatten()
                .filter_map(|s| match s {
                    Segment::Unknown { guess, .. } => Some(guess.clone()),
                    _ => None,
                })
                .collect::<BTreeSet<String>>(),
        ),
        _ => None,
    }) else {
        return Err("evlar is not a hybrid reading".into());
    };
    assert!(guesses.contains("N vow=bu") && guesses.contains("ADJ vow=bu"), "{guesses:?}");
    Ok(())
}

#[test]
fn unknown_stems_infer_their_exception() -> TestResult {
    // left: Turkish loanword with INDUCE
    let tr = Lexer::compile(&(languages::TURKISH.to_string() + "\nINDUCE vow IN Nouns Adjs\n"))?;
    assert_eq!(tr.lex("googlelar", Mode::Hybrid).render(), "⟨google⟩<?N vow=bu>{g ##o ##o ##g ##le} + lAr<PL>");
    assert_eq!(tr.lex("googlelerden", Mode::Hybrid).render(), "⟨google⟩<?N>{g ##o ##o ##g ##le} + lAr<PL> + DAn<ABL>");
    // without INDUCE the suffix cannot attach to a front-final unknown stem
    let plain = Lexer::compile(languages::TURKISH)?;
    let out = plain.lex("googlelar", Mode::Hybrid);
    assert!(!out.tokens().iter().any(|t| t.ends_with("<PL>")), "{}", out.render());
    // right: a prefix that disagrees with an unknown root's vowels
    let ch = Lexer::compile(CHAINS_INDUCE)?;
    assert_eq!(ch.lex("betot", Mode::Hybrid).render(), "bA<P1> + ⟨tot⟩<?S h=p>{t ##o ##t}");
    assert_eq!(ch.lex("batot", Mode::Hybrid).render(), "bA<P1> + ⟨tot⟩<?S>{t ##o ##t}");
    // known words never change
    for w in ["betik", "batok", "tik"] {
        assert_eq!(ch.lex(w, Mode::Hybrid), ch.lex(w, Mode::Strict));
    }
    Ok(())
}

#[test]
fn oracle_on_new_specs() -> TestResult {
    let glosses = |out: Option<WordOutput>, depth: usize| -> BTreeSet<Gloss> {
        match out {
            Some(WordOutput::Analyzed { all, .. }) => {
                all.into_iter().filter(|a| a.len() <= depth).map(|a| a.into_iter().map(|m| (m.lemma, m.tag)).collect()).collect()
            }
            _ => BTreeSet::new(),
        }
    };
    let mut rng = Rng::new(23);
    for (name, text) in [("opaque", OPAQUE), ("chains_induce", CHAINS_INDUCE), ("pins_induce", PINS_INDUCE)] {
        let gen = oracle::generate(&parse_spec(text)?, 4, 100_000)?;
        let f = lexer(text)?;
        for (surface, want) in &gen.surfaces {
            assert_eq!(&glosses(f.all_analyses(surface, 512), 4), want, "{name}: {surface}");
        }
        for w in gen.naive.iter().chain(&gen.wrong_archi) {
            if !gen.surfaces.contains_key(w) {
                assert!(glosses(f.all_analyses(w, 512), 4).is_empty(), "{name} accepted forbidden {w}");
            }
        }
        let good: Vec<&String> = gen.surfaces.keys().collect();
        let alpha: Vec<char> = "abdeiklmnoprtuïf".chars().collect();
        for i in 0..5000 {
            let w: String = if i % 2 == 0 {
                rng.pick(&good).to_string()
            } else {
                (0..1 + rng.below(9)).map(|_| *rng.pick(&alpha)).collect()
            };
            if matches!(f.lex(&w, Mode::Strict), WordOutput::Analyzed { .. }) {
                assert_eq!(f.lex(&w, Mode::Hybrid), f.lex(&w, Mode::Strict), "{name}: {w}");
            }
        }
        let bytes = f.to_bytes();
        assert_eq!(Lexer::from_bytes(&bytes)?, f, "{name}");
        for _ in 0..60 {
            let mut bad = bytes.clone();
            let k = rng.below(bad.len());
            bad[k] ^= 1 << rng.below(8);
            assert!(Lexer::from_bytes(&bad).is_err());
        }
    }
    Ok(())
}
