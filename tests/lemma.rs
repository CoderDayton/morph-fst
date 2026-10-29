//! One best reading per word: its lemma, and readings without repeats.

use morph_fst::{Kind, Lexer};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn lemmas(words: &[&str]) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let lx = Lexer::bundled("english")?;
    Ok(words.iter().map(|w| lx.analyze(w).lemma().to_string()).collect())
}

#[test]
fn inflection_is_stripped() -> TestResult {
    assert_eq!(lemmas(&["runners", "happier", "walked", "left", "costs"])?, ["runner", "happy", "walk", "leave", "cost"]);
    Ok(())
}

#[test]
fn derivation_stays_in_the_lemma() -> TestResult {
    assert_eq!(lemmas(&["really", "helpful", "government", "unhappiest", "rerunning"])?, ["really", "helpful", "government", "unhappy", "rerun"]);
    Ok(())
}

#[test]
fn closed_class_words_have_dictionary_lemmas() -> TestResult {
    assert_eq!(lemmas(&["me", "them", "an", "these", "as"])?, ["i", "they", "a", "this", "as"]);
    Ok(())
}

#[test]
fn guessed_words_and_non_words_have_lemmas_too() -> TestResult {
    let lx = Lexer::bundled("english")?;
    let guessed = lx.analyze("blorfing");
    assert_eq!(guessed.kind(), Kind::Guessed);
    assert_eq!(guessed.lemma(), "blorf");
    let words = lx.analyze_text("x@y.com");
    assert_eq!(words.first().map(|w| w.lemma()), Some("x@y.com"));
    Ok(())
}

const SPEC: &str = "LEXICON Root\n  help V Suffix ;\n\nLEXICON Suffix\n  ful DER # ;\n  s PL # ;\n  0 # ;\n";

#[test]
fn the_grammar_says_which_endings_are_derivation() -> TestResult {
    let plain = Lexer::compile(SPEC)?;
    assert_eq!(plain.analyze("helpful").lemma(), "help");

    let marked = Lexer::compile(&format!("OPTION derivation DER\n{SPEC}"))?;
    assert_eq!(marked.analyze("helpful").lemma(), "helpful");
    assert_eq!(marked.analyze("helps").lemma(), "help");

    let saved = Lexer::from_bytes(&marked.to_bytes())?;
    assert_eq!(saved.analyze("helpful").lemma(), "helpful");
    Ok(())
}

#[test]
fn a_derivation_tag_matches_its_sub_tags_only() -> TestResult {
    let spec = SPEC.replace("ful DER", "ful DER:ADJ").replace("s PL", "s DERIVED");
    let lx = Lexer::compile(&format!("OPTION derivation DER\n{spec}"))?;
    assert_eq!(lx.analyze("helpful").lemma(), "helpful");
    assert_eq!(lx.analyze("helps").lemma(), "help");
    Ok(())
}

#[test]
fn a_derivation_list_must_not_be_empty() {
    assert!(Lexer::compile(&format!("OPTION derivation ,\n{SPEC}")).is_err());
}

#[test]
fn readings_do_not_repeat() -> TestResult {
    let lx = Lexer::bundled("english")?;
    for w in ["costs", "flies", "leaves", "rerunning", "sites"] {
        let word = lx.analyze(w);
        let mut all = vec![word.parts().to_vec()];
        all.extend(word.alternatives().iter().cloned());
        for (i, a) in all.iter().enumerate() {
            assert!(!all.iter().skip(i + 1).any(|b| a == b), "{w}: repeated reading {a:?}");
        }
    }
    Ok(())
}
