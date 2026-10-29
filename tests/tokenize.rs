//! The tokenizer: lossless spans, whole emails/URLs/numbers, clitics, abbreviations.

use morph_fst::{tokenize, Class, Kind, Lexer};

const TEXT: &str = "Dr. Smith's e-mail: x@y.com costs $4.50! Don't visit https://a.b/c?d=1, e.g. U.S. sites... 1,000 (3rd)";

fn texts(text: &str) -> Vec<&str> {
    tokenize(text).iter().map(|t| &text[t.span()]).collect()
}

#[test]
fn messy_english_splits_the_way_a_reader_would() {
    assert_eq!(
        texts(TEXT),
        [
            "Dr.", "Smith", "'s", "e", "-", "mail", ":", "x@y.com", "costs", "$", "4.50", "!", "Do", "n't", "visit",
            "https://a.b/c?d=1", ",", "e.g.", "U.S.", "sites", "...", "1,000", "(", "3rd", ")"
        ]
    );
}

#[test]
fn every_token_has_a_class() {
    let class = |s: &str| tokenize(s).first().map(|t| t.class());
    assert_eq!(class("x@y.com"), Some(Class::Email));
    assert_eq!(class("https://a.b"), Some(Class::Url));
    assert_eq!(class("www.example.org/x"), Some(Class::Url));
    assert_eq!(class("4.50"), Some(Class::Number));
    assert_eq!(class("r2d2"), Some(Class::Number));
    assert_eq!(class("Dr."), Some(Class::Abbreviation));
    assert_eq!(tokenize("Smith's").get(1).map(|t| t.class()), Some(Class::Clitic));
    assert_eq!(class("'s"), Some(Class::Punct));
    assert_eq!(class("..."), Some(Class::Punct));
    assert_eq!(class("$"), Some(Class::Symbol));
    assert_eq!(class("dogs"), Some(Class::Word));
}

#[test]
fn a_sentence_final_period_is_its_own_token() {
    assert_eq!(texts("It barked. Then it ran."), ["It", "barked", ".", "Then", "it", "ran", "."]);
}

#[test]
fn trailing_punctuation_is_not_part_of_a_url_or_email() {
    assert_eq!(texts("(see http://a.b/c). Mail me@x.org, ok"), ["(", "see", "http://a.b/c", ")", ".", "Mail", "me@x.org", ",", "ok"]);
}

#[test]
fn clitics_and_inner_apostrophes() {
    assert_eq!(texts("can't we're o'clock y’all I'm"), ["ca", "n't", "we", "'re", "o'clock", "y’all", "I", "'m"]);
}

#[test]
fn tokens_are_lossless_for_any_text() {
    let alphabet: Vec<char> = "ab Z9.,'’-@:/$%…\n\t\u{301}é日😀\u{FE0F}\u{200D}(".chars().collect();
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    for _ in 0..4000 {
        let mut text = String::new();
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        for _ in 0..(seed >> 59) {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            text.push(alphabet[(seed >> 33) as usize % alphabet.len()]);
        }
        let mut at = 0;
        for t in tokenize(&text) {
            let span = t.span();
            assert!(span.start >= at && span.end > span.start, "{text:?}: out of order at {span:?}");
            assert!(text.get(at..span.start).is_some_and(|gap| gap.chars().all(char::is_whitespace)), "{text:?}: dropped text before {span:?}");
            assert!(text.get(span.clone()).is_some_and(|s| !s.chars().any(char::is_whitespace)), "{text:?}: whitespace inside {span:?}");
            at = span.end;
        }
        assert!(text.get(at..).is_some_and(|rest| rest.chars().all(char::is_whitespace)), "{text:?}: dropped tail");
    }
}

#[test]
fn analyzed_words_point_back_into_the_original_text() -> Result<(), Box<dyn std::error::Error>> {
    let lx = Lexer::bundled("english")?;
    let words = lx.analyze_text(TEXT);
    let originals = texts(TEXT);
    assert_eq!(words.len(), originals.len());
    for (w, o) in words.iter().zip(&originals) {
        assert_eq!(&TEXT[w.span()], *o);
    }
    let by = |t: &str| words.iter().find(|w| &TEXT[w.span()] == t);
    assert_eq!(by("x@y.com").map(|w| w.kind()), Some(Kind::Literal));
    assert_eq!(by("x@y.com").and_then(|w| w.parts().first().map(|p| p.tag().to_string())), Some("EMAIL".into()));
    assert_eq!(by("4.50").map(|w| w.kind()), Some(Kind::Literal));
    assert_eq!(by("costs").map(|w| w.kind()), Some(Kind::Known));
    assert_eq!(by("Smith").map(|w| w.text()), Some("smith"));
    Ok(())
}
