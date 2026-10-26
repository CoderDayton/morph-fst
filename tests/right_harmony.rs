//! Right-to-left (regressive) context: RFEATURE registers, archiphonemes
//! reading right context, right pins (`@rset`), and the Igbo demo.
//! Synthetic toy specs stress the mechanism; the independent oracle is
//! checked on all of them.

use morph_fst::fused::{derive_vocab, FusedLexer, Mode};
use morph_fst::normalize::normalize;
use morph_fst::oracle::{self, Gloss};
use morph_fst::output::{analysis_string, WordOutput};
use morph_fst::rng::Rng;
use morph_fst::spec::{self, parse_spec};
use morph_fst::WordLexer;
use std::collections::BTreeSet;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const IGBO: &str = include_str!("../data/igbo.morph");

/// Long prefix chains: every prefix agrees with the nearest vowel to its
/// right, through the other prefixes. A vowelless stem leaves them `none`.
const CHAINS: &str = r#"
CLASS PLUS = i e
CLASS MINUS = a o
RFEATURE h
  [PLUS] -> p
  [MINUS] -> m
ARCHI A
  h=m -> a
  * -> e
LEXICON Root
  0 P1 ;
LEXICON P1
  b{A}  P1  P2 ;
  0 P2 ;
LEXICON P2
  d{A}  P2  P3 ;
  0 P3 ;
LEXICON P3
  g{A}  P3  Stems ;
  0 Stems ;
LEXICON Stems @open=S
  tik   S  # ;
  tok   S  # ;
  k     S  # ;
"#;

/// Dominant harmony: a dominant vowel ü anywhere to the right turns every
/// {U} to its left into ü (sticky right register), across morphemes.
const DOMINANT: &str = r#"
RFEATURE dom
  <dom=y> [*] -> y
  ü -> y
ARCHI U
  dom=y -> ü
  * -> u
LEXICON Root
  0 Pfx ;
LEXICON Pfx
  b{U}   P   Stems ;
  0 Stems ;
LEXICON Stems
  k{U}t  S   Sfx ;
  m{U}r  S   Sfx ;
LEXICON Sfx
  ün     DOM  # ;
  an     NEU  # ;
  0 # ;
"#;

/// Exceptions in both directions: `pil` has a +ATR-looking vowel but acts
/// −ATR towards its prefixes (@rset) and its suffixes (@set). Epsilons in
/// between must carry pins unchanged.
const PINS: &str = r#"
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
LEXICON Root
  0 P1 ;
LEXICON P1
  b{A}  P1  P2 ;
  0 P2 ;
LEXICON P2
  d{A}  P2  Mid ;
  0 Mid ;
LEXICON Mid
  0 Stems ;
LEXICON Stems
  tik   S  Sfx ;
  tok   S  Sfx ;
  pil   S  Sfx @rset:h=m @set:lf=m ;
LEXICON Sfx
  0 Sfx2 ;
LEXICON Sfx2
  n{O}  N  # ;
  0 # ;
"#;

/// Bidirectional affixes around a root, and a linker that reads both sides.
const BOTH: &str = r#"
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
ARCHI L
  lf=m h=m -> a
  lf=p h=p -> e
  * -> u
LEXICON Root
  0 Pfx ;
LEXICON Pfx
  b{A}  P  Stems ;
  0 Stems ;
LEXICON Stems
  tik   S  Link ;
  tok   S  Link ;
LEXICON Link
  {L}   LNK  Stems2 ;
  0 Sfx ;
LEXICON Stems2
  tik   S  Sfx ;
  tok   S  Sfx ;
LEXICON Sfx
  s{O}  X  # ;
  0 # ;
"#;

/// A cross-morpheme cycle: prefix reads the right, suffix reads the left,
/// with a vowelless stem between — two consistent surfaces, both accepted.
const CYCLE: &str = r#"
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
LEXICON Root
  b{A}  P  Stem ;
LEXICON Stem
  k     S  Sfx ;
LEXICON Sfx
  s{O}  X  # ;
"#;

fn lexer(text: &str) -> morph_fst::Result<FusedLexer> {
    FusedLexer::compile(text)
}

fn accepts(f: &FusedLexer, w: &str) -> bool {
    matches!(f.lex_word(w), WordOutput::Analyzed { .. })
}

fn check_sets(text: &str, yes: &[&str], no: &[&str]) -> TestResult {
    let f = lexer(text)?;
    for w in yes {
        assert!(accepts(&f, w), "should accept {w}");
    }
    for w in no {
        assert!(!accepts(&f, w), "should reject {w}");
    }
    Ok(())
}

#[test]
fn prefix_chains_agree_with_the_root() -> TestResult {
    check_sets(
        CHAINS,
        &["tik", "tok", "betik", "batok", "bedegetik", "badagatok", "degetik", "bagatok", "k", "bedegek"],
        &["bedegatok", "badagatik", "batik", "betok", "bagetok", "badegetok", "bak"],
    )
}

#[test]
fn dominant_harmony_spreads_leftward() -> TestResult {
    check_sets(
        DOMINANT,
        &["kut", "kutan", "kütün", "bukut", "bukutan", "bükütün", "mürün", "bümürün"],
        &["kutün", "bukütün", "bükutün", "kütan", "büküt", "bükütan"],
    )
}

#[test]
fn right_and_left_pins() -> TestResult {
    check_sets(
        PINS,
        &["pil", "bapil", "badapil", "dapil", "pilno", "bapilno", "tik", "betik", "bedetik", "tikni", "tok", "batok", "tokno"],
        &["bepil", "bedepil", "depil", "pilni", "bepilno", "batik", "tikno", "bedapil"],
    )?;
    // Guessed right pins never duplicate an analysis.
    let f = lexer(PINS)?;
    for w in ["pil", "bapil", "badapil", "bapilno", "bedetik", "tokno"] {
        match f.lex_word(w) {
            WordOutput::Analyzed { all, .. } => {
                let set: BTreeSet<String> = all.iter().map(analysis_string).collect();
                assert_eq!(set.len(), all.len(), "{w}: duplicate analyses {all:?}");
            }
            o => panic!("{w}: {}", o.render()),
        }
    }
    Ok(())
}

#[test]
fn bidirectional_and_both_ways_linker() -> TestResult {
    check_sets(
        BOTH,
        &["tik", "betik", "batok", "tiksi", "tokso", "tiketik", "tokatok", "tikutok", "tokutik", "betiketik", "batokatok", "betikutok", "betikutokso", "batokutiksi"],
        &["batik", "betok", "tikso", "toksi", "tikatik", "tiketok", "tokutok", "batikutok", "betikutoksi"],
    )
}

#[test]
fn cross_morpheme_cycle_has_two_consistent_surfaces() -> TestResult {
    check_sets(CYCLE, &["bakso", "beksi"], &["baksi", "bekso"])
}

#[test]
fn compile_errors() {
    let cases = [
        (
            "RFEATURE h\n a -> m\nFEATURE f\n a -> m\nARCHI R\n h=m -> a\n * -> e\nARCHI L\n f=m -> a\n * -> e\nLEXICON Root\n {R}x{L} N # ;",
            "follows one that reads right context",
        ),
        (
            "RFEATURE h\n a -> m\nARCHI A\n h=m -> a\n * -> e\nLEXICON Root\n {A}tak N S ;\nLEXICON S\n x X # ;\nRULES\n r: k -> g / _ +{x} ;",
            "reads as right context",
        ),
        ("RFEATURE h\n a -> m\nLEXICON Root\n x N # @set:h=m ;", "use @set for FEATUREs"),
        ("FEATURE f\n a -> m\nLEXICON Root\n x N # @rset:f=m ;", "use @set for FEATUREs"),
        ("FEATURE f\n <h=m> a -> m\nRFEATURE h\n a -> m\nLEXICON Root\n x N # ;", "may not test the right-context"),
        ("RFEATURE h\n <f=m> a -> m\nFEATURE f\n a -> m\nLEXICON Root\n x N # ;", "may not test the left-context"),
        ("RFEATURE h\n a -> m\nARCHI A\n h=m -> a\nLEXICON Root\n {A} N # ;", "no realisation"),
    ];
    for (src, needle) in cases {
        match spec::load(src) {
            Err(e) => assert!(e.to_string().contains(needle), "{src:?}: got `{e}`, want `{needle}`"),
            Ok(_) => panic!("{src:?} should fail"),
        }
    }
}

#[test]
fn igbo_golden() -> TestResult {
    let f = lexer(IGBO)?;
    let cases = [
        ("iri", "I<INF>+ri<V:eat>"),
        ("ịga", "I<INF>+ga<V:go>"),
        ("ịbịa", "I<INF>+bịa<V:come>"),
        ("inye", "I<INF>+nye<V:give>"),
        ("ime", "I<INF>+me<V:do>"),
        ("ikwu", "I<INF>+kwu<V:say>"),
        ("ịhụ", "I<INF>+hụ<V:see>"),
        ("ịzụ", "I<INF>+zụ<V:buy>"),
        ("ịchọ", "I<INF>+chọ<V:want>"),
        ("ịsa", "I<INF>+sa<V:wash>"),
        ("ịṅụ", "I<INF>+ṅụ<V:drink>"),
        ("eri", "E<PTCP>+ri<V:eat>"),
        ("eme", "E<PTCP>+me<V:do>"),
        ("aga", "E<PTCP>+ga<V:go>"),
        ("abịa", "E<PTCP>+bịa<V:come>"),
        ("riri", "ri<V:eat>+rV<PST>"),
        ("gara", "ga<V:go>+rV<PST>"),
        ("bịara", "bịa<V:come>+rV<PST>"),
        ("nyere", "nye<V:give>+rV<PST>"),
        ("mere", "me<V:do>+rV<PST>"),
        ("kwuru", "kwu<V:say>+rV<PST>"),
        ("dere", "de<V:write>+rV<PST>"),
        ("hụrụ", "hụ<V:see>+rV<PST>"),
        ("zụrụ", "zụ<V:buy>+rV<PST>"),
        ("chọrọ", "chọ<V:want>+rV<PST>"),
        ("sara", "sa<V:wash>+rV<PST>"),
        ("ṅụrụ", "ṅụ<V:drink>+rV<PST>"),
        ("gaghị", "ga<V:go>+ghị<NEG>"),
        ("erighị", "E<NEG.IND>+ri<V:eat>+ghị<NEG>"),
        ("agaghị", "E<NEG.IND>+ga<V:go>+ghị<NEG>"),
    ];
    for (w, a) in cases {
        match f.lex_word(w) {
            WordOutput::Analyzed { best, .. } => assert_eq!(analysis_string(&best), a, "{w}"),
            o => panic!("{w} not analyzed: {}", o.render()),
        }
    }
    for w in ["ịri", "iga", "abịara", "egara", "ara", "rira", "gari", "ri", "ịkwu", "ehụ", "ịnye", "gbaghi"] {
        assert!(!accepts(&f, w), "{w} should fall back");
    }
    // hybrid: an unknown root still selects its prefix from its own vowels
    assert_eq!(f.lex("ịkpụ", Mode::Hybrid).render(), "I<INF> + ⟨kpụ⟩<?V>{k ##pụ}");
    assert!(matches!(f.lex("ifọ", Mode::Hybrid), WordOutput::Fallback(_)));
    // tone marks are stripped, dots kept (precomposed or combining)
    for (raw, clean) in [("ị́ga", "ịga"), ("i\u{323}\u{301}ga", "ịga"), ("i\u{301}\u{323}ga", "ịga"), ("írí", "iri"), ("ṅụ̀rụ̀", "ṅụrụ")] {
        assert_eq!(normalize(raw, &f.mt.norm), clean, "{raw:?}");
    }
    let out = morph_fst::lex_text(&f, "Ị́ga na-eri");
    assert_eq!(out[0].0, "ịga");
    assert!(matches!(out[0].1, WordOutput::Analyzed { .. }));
    Ok(())
}

/// The independent oracle on Igbo and every toy spec.
#[test]
fn oracle_right_context() -> TestResult {
    let glosses = |out: Option<WordOutput>, depth: usize| -> BTreeSet<Gloss> {
        match out {
            Some(WordOutput::Analyzed { all, .. }) => {
                all.into_iter().filter(|a| a.len() <= depth).map(|a| a.into_iter().map(|m| (m.lemma, m.tag)).collect()).collect()
            }
            _ => BTreeSet::new(),
        }
    };
    for (name, text, depth, min_pos, min_neg) in
        [("igbo", IGBO, 3, 100, 50), ("chains", CHAINS, 4, 20, 10), ("dominant", DOMINANT, 3, 10, 5), ("pins", PINS, 4, 20, 10), ("both", BOTH, 5, 20, 10), ("cycle", CYCLE, 3, 2, 1)]
    {
        let gen = oracle::generate(&parse_spec(text)?, depth, 1_000_000)?;
        let f = lexer(text)?;
        for (surface, want) in &gen.surfaces {
            assert_eq!(&glosses(f.analyze(surface, 512), depth), want, "{name}: {surface}");
        }
        let mut neg = 0;
        for w in gen.naive.iter().chain(&gen.wrong_archi) {
            if gen.surfaces.contains_key(w) {
                continue;
            }
            assert!(glosses(f.analyze(w, 512), depth).is_empty(), "{name} accepted forbidden {w}");
            neg += 1;
        }
        eprintln!("{name}: {} surfaces, {neg} forbidden forms rejected", gen.surfaces.len());
        assert!(gen.surfaces.len() >= min_pos && neg >= min_neg, "{name}: oracle too small ({}, {neg})", gen.surfaces.len());
    }
    Ok(())
}

#[test]
fn hybrid_matches_strict_and_artifacts_round_trip() -> TestResult {
    let mut rng = Rng::new(17);
    for text in [IGBO, CHAINS, DOMINANT, PINS, BOTH, CYCLE] {
        let f = lexer(text)?;
        let good: Vec<String> = oracle::generate(&parse_spec(text)?, 4, 100_000)?.surfaces.into_keys().collect();
        let alpha: Vec<char> = derive_vocab(&spec::load(text)?)
            .iter()
            .filter(|p| !p.cont && p.text.chars().count() == 1)
            .filter_map(|p| p.text.chars().next())
            .chain(['😀'])
            .collect();
        for i in 0..8000 {
            let w: String = match i % 3 {
                0 => rng.pick(&good).clone(),
                1 => {
                    let mut cs: Vec<char> = rng.pick(&good).chars().collect();
                    let k = rng.below(cs.len());
                    cs[k] = *rng.pick(&alpha);
                    cs.into_iter().collect()
                }
                _ => (0..rng.below(10)).map(|_| *rng.pick(&alpha)).collect(),
            };
            if matches!(f.lex_word(&w), WordOutput::Analyzed { .. }) {
                assert_eq!(f.lex(&w, Mode::Hybrid), f.lex_word(&w), "{w}");
            }
        }
        let bytes = f.to_bytes();
        assert_eq!(bytes, FusedLexer::compile(text)?.to_bytes());
        assert_eq!(FusedLexer::from_bytes(&bytes)?, f);
        for _ in 0..80 {
            let mut bad = bytes.clone();
            let k = rng.below(bad.len());
            bad[k] ^= 1 << rng.below(8);
            assert!(FusedLexer::from_bytes(&bad).is_err());
            assert!(FusedLexer::from_bytes(&bytes[..rng.below(bytes.len())]).is_err());
        }
    }
    Ok(())
}
