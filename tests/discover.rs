//! Drafting a grammar from a bare word list, with no labels.

use morph_fst::{discover, Lexer, Mode, Segment, WordOutput};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const STEMS: [&str; 8] = ["walk", "talk", "jump", "look", "play", "work", "call", "help"];
const ENDINGS: [&str; 4] = ["", "s", "ed", "ing"];

fn words() -> String {
    let mut out = String::new();
    for s in STEMS {
        for e in ENDINGS {
            out.push_str(&format!("{s}{e}\n"));
        }
    }
    out.push_str("the\nand\n");
    out
}

#[test]
fn every_listed_word_is_analyzed() -> TestResult {
    let lx = Lexer::from_wordlist(&words())?;
    for w in words().lines() {
        assert!(matches!(lx.lex(w, Mode::Strict), WordOutput::Analyzed { .. }), "{w}");
    }
    Ok(())
}

#[test]
fn an_unseen_stem_takes_the_discovered_endings() -> TestResult {
    let lx = Lexer::from_wordlist(&words())?;
    match lx.lex("kicked", Mode::Hybrid) {
        WordOutput::Hybrid { segments, .. } => {
            assert!(matches!(&segments[0], Segment::Unknown { surface, .. } if surface == "kick"), "{segments:?}");
            assert!(matches!(segments.last(), Some(Segment::Morph(m)) if m.surface == "ed"), "{segments:?}");
        }
        other => panic!("{other:?}"),
    }
    Ok(())
}

#[test]
fn the_draft_is_deterministic_and_reports_its_findings() -> TestResult {
    let a = discover::from_wordlist(&words())?;
    assert_eq!(a, discover::from_wordlist(&words())?);
    assert_eq!(a.words, 34);
    assert!(a.suffixes.iter().any(|s| s == "ing"), "{:?}", a.suffixes);
    assert!(a.paradigms >= 1);
    Ok(())
}

#[test]
fn numbers_and_symbols_are_not_words() -> TestResult {
    let noisy = format!("{}r2d2\n1990s\nwalk-ed\n", words());
    assert_eq!(discover::from_wordlist(&noisy)?.words, 34);
    Ok(())
}

#[test]
fn endings_stacked_in_the_list_are_split_into_their_pieces() -> TestResult {
    let mut list = String::new();
    for s in ["masa", "kapı", "yol", "dere", "kitap", "araba", "okul", "deniz"] {
        for e in ["", "lar", "da", "larda"] {
            list.push_str(&format!("{s}{e}\n"));
        }
    }
    list.push_str("kalem\nkalemlar\nkalemda\n");
    let lx = Lexer::from_wordlist(&list)?;
    let word = lx.analyze("kalemlarda");
    let texts: Vec<&str> = word.parts().iter().map(|p| p.text()).collect();
    assert_eq!(texts, ["kalem", "lar", "da"], "{word}");
    Ok(())
}

#[test]
fn vowel_harmony_variants_share_one_tag() -> TestResult {
    let mut list = String::new();
    for s in ["masa", "kapı", "yol", "dere", "kitap", "araba", "okul", "deniz"] {
        for e in ["", "lar", "da"] {
            list.push_str(&format!("{s}{e}\n"));
        }
    }
    for s in ["elma", "ders", "göz", "gül", "kedi", "süt", "köy", "kül"] {
        for e in ["", "ler", "de"] {
            list.push_str(&format!("{s}{e}\n"));
        }
    }
    let lx = Lexer::from_wordlist(&list)?;
    let tag = |w: &str| lx.analyze(w).parts().last().map(|p| p.tag().to_string());
    assert!(tag("evler").is_some());
    assert_eq!(tag("evler"), tag("masalar"));
    Ok(())
}

#[test]
fn a_list_with_nothing_to_find_is_an_error() {
    assert!(Lexer::from_wordlist("").is_err());
    assert!(Lexer::from_wordlist("   \n\t\n").is_err());
}
