//! The everyday API: `analyze`, `Word`, and the language builder.

use morph_fst::{Kind, Lexer, Mode};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const WALK_TALK: &str = "walk\twalk\tV;NFIN\nwalk\twalked\tV;PST\nwalk\twalks\tV;PRS;3;SG\n\
                         talk\ttalk\tV;NFIN\ntalk\ttalked\tV;PST\ntalk\ttalks\tV;PRS;3;SG\n";
const EXTRA: &str = "kick kicks kicked kicking jump jumps jumped jumping look looks looked looking";

#[test]
fn a_known_word_has_parts_with_lemma_and_tag() -> TestResult {
    let lx = Lexer::bundled("turkish")?;
    let w = lx.analyze("evlerde");
    assert_eq!(w.kind(), Kind::Known);
    assert_eq!(w.text(), "evlerde");
    let tags: Vec<&str> = w.parts().iter().map(|p| p.tag()).collect();
    assert_eq!(tags, ["N", "PL", "LOC"]);
    assert!(w.parts().iter().all(|p| p.is_known()));
    assert_eq!(w.parts()[0].lemma(), "ev");
    assert_eq!(w.parts()[1].text(), "ler");
    assert!(!w.to_string().is_empty());
    Ok(())
}

#[test]
fn an_unknown_word_is_guessed_by_default() -> TestResult {
    let lx = Lexer::bundled("english")?;
    let w = lx.analyze("blorfing");
    assert_eq!(w.kind(), Kind::Guessed);
    let first = &w.parts()[0];
    assert!(!first.is_known());
    assert_eq!(first.text(), "blorf");
    assert_eq!(w.parts().last().map(|p| p.text()), Some("ing"));
    assert!(!w.alternatives().is_empty());
    Ok(())
}

#[test]
fn strict_mode_splits_instead_of_guessing() -> TestResult {
    let lx = Lexer::bundled("english")?.mode(Mode::Strict);
    let w = lx.analyze("blorfq");
    assert_eq!(w.kind(), Kind::Unsplit);
    let joined: String = w.parts().iter().map(|p| p.text()).collect();
    assert_eq!(joined, "blorfq");
    Ok(())
}

#[test]
fn text_is_normalized_and_split_into_words() -> TestResult {
    let lx = Lexer::bundled("turkish")?;
    let words = lx.analyze_text("IŞIK Evlerde, saatler");
    let texts: Vec<&str> = words.iter().map(|w| w.text()).collect();
    assert_eq!(texts, ["ışık", "evlerde", ",", "saatler"]);
    Ok(())
}

#[test]
fn a_language_comes_from_tables_plus_extra_words() -> TestResult {
    let tables_only = Lexer::builder().unimorph(WALK_TALK).build()?;
    assert_eq!(tables_only.analyze("talked").kind(), Kind::Known);
    assert_eq!(tables_only.analyze("kicked").kind(), Kind::Guessed);

    let both = Lexer::builder().unimorph(WALK_TALK).words(EXTRA).build()?;
    assert_eq!(both.analyze("talked").kind(), Kind::Known);
    assert_eq!(both.analyze("kicked").kind(), Kind::Known);
    // an unseen stem still borrows the discovered endings
    assert_eq!(both.analyze("pushed").kind(), Kind::Guessed);
    Ok(())
}

#[test]
fn a_builder_needs_something_to_build_from() {
    assert!(Lexer::builder().build().is_err());
    assert!(Lexer::builder().words("   ").build().is_err());
    assert!(Lexer::builder().corpus("some text").build().is_err());
}

#[test]
fn a_lexer_survives_a_round_trip_through_a_file() -> TestResult {
    let lx = Lexer::bundled("turkish")?;
    let path = std::env::temp_dir().join(format!("morph-fst-friendly-{}.mcfu", std::process::id()));
    lx.save(&path)?;
    let back = Lexer::load(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(back?, lx);
    assert!(Lexer::load(&path).is_err());
    Ok(())
}
