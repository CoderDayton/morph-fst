//! Zero morphemes: `0 TAG Cont ;` is a morpheme with an empty surface.

use morph_fst::oracle::{self, Gloss};
use morph_fst::output::segmentation;
use morph_fst::spec::parse_spec;
use morph_fst::{Lexer, Mode, WordOutput};
use std::collections::BTreeSet;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SPEC: &str = "
LEXICON Root
  0 Nouns ;
  0 Verbs ;
LEXICON Nouns @open=N
  cat N Num ;
  box N Num ;
  fox N Num ;
LEXICON Verbs @open=V
  walk V VInfl ;
  fix V VInfl ;
LEXICON Num
  s PL Case ;
  0 SG Case ;
LEXICON Case
  0 NOM # ;
  z GEN # ;
LEXICON VInfl
  s 3SG # ;
  ed PST # ;
  0 NFIN # ;
RULES
  x_es: x -> xe / _ +{s,z} ;
";

fn tokens(lx: &Lexer, w: &str) -> Vec<String> {
    lx.lex(w, Mode::Strict).tokens()
}

#[test]
fn bare_form_carries_zero_tags() -> TestResult {
    let lx = Lexer::compile(SPEC)?;
    assert_eq!(tokens(&lx, "cat"), ["cat<N>", "<SG>", "<NOM>"]);
    assert_eq!(tokens(&lx, "walk"), ["walk<V>", "<NFIN>"]);
    let WordOutput::Analyzed { best, .. } = lx.lex("cat", Mode::Strict) else { return Err("cat rejected".into()) };
    assert_eq!(segmentation(&best), "cat");
    assert!(best[1..].iter().all(|m| m.start == 3 && m.end == 3 && m.surface.is_empty()));
    Ok(())
}

#[test]
fn overt_suffixes_and_rules_see_through_zeros() -> TestResult {
    let lx = Lexer::compile(SPEC)?;
    assert_eq!(tokens(&lx, "cats"), ["cat<N>", "s<PL>", "<NOM>"]);
    assert_eq!(tokens(&lx, "catz"), ["cat<N>", "<SG>", "z<GEN>"]);
    // The rule's right context is the next overt morpheme, past the zero.
    assert_eq!(tokens(&lx, "boxez"), ["box<N>", "<SG>", "z<GEN>"]);
    assert_eq!(tokens(&lx, "fixes"), ["fix<V>", "s<3SG>"]);
    assert!(matches!(lx.lex("boxz", Mode::Strict), WordOutput::Fallback(_)));
    Ok(())
}

#[test]
fn zeros_survive_save_and_load() -> TestResult {
    let lx = Lexer::compile(SPEC)?;
    let back = Lexer::from_bytes(&lx.to_bytes())?;
    assert_eq!(back, lx);
    assert_eq!(tokens(&back, "fox"), ["fox<N>", "<SG>", "<NOM>"]);
    Ok(())
}

#[test]
fn misplaced_zeros_are_errors() {
    let err = |s: &str| Lexer::compile(s).err().map(|e| e.to_string()).unwrap_or_default();
    let before_any = "LEXICON Root\n  0 X A ;\nLEXICON A\n  a A # ;\n";
    assert!(err(before_any).contains("must follow a morpheme"), "{}", err(before_any));
    let cycle = "LEXICON Root\n  a A L ;\nLEXICON L\n  0 X M ;\nLEXICON M\n  0 L ;\n  b B # ;\n";
    assert!(err(cycle).contains("cycle"), "{}", err(cycle));
    let flagged = "LEXICON Root\n  a A L ;\nLEXICON L\n  0 X # @f ;\n";
    assert!(err(flagged).contains("no flags"), "{}", err(flagged));
}

#[test]
fn analyzer_matches_oracle_with_zeros() -> TestResult {
    let spec = parse_spec(SPEC)?;
    let gen = oracle::generate(&spec, 4, 100_000)?;
    let lx = Lexer::compile(SPEC)?;
    assert!(gen.surfaces.len() > 10);
    for (surface, want) in &gen.surfaces {
        let got: BTreeSet<Gloss> = match lx.all_analyses(surface, 512) {
            Some(WordOutput::Analyzed { all, .. }) => all.into_iter().map(|a| a.into_iter().map(|m| (m.lemma, m.tag)).collect()).collect(),
            _ => BTreeSet::new(),
        };
        assert_eq!(&got, want, "{surface}");
    }
    Ok(())
}
