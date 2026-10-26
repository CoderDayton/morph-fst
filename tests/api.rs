//! The public API with default features (no dependencies).

use morph_fst::output::{reconstruct, segmentation};
use morph_fst::rng::Rng;
use morph_fst::{languages, Error, Lexer, Mode, Segment, WordOutput};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn best(lx: &Lexer, w: &str) -> Option<(Vec<String>, String)> {
    match lx.lex(w, Mode::Strict) {
        WordOutput::Analyzed { best, .. } => Some((best.iter().map(|m| format!("{}<{}>", m.lemma, m.tag)).collect(), segmentation(&best))),
        _ => None,
    }
}

#[test]
fn every_bundled_language_compiles_and_round_trips() -> TestResult {
    for (name, src) in languages::ALL {
        let lx = Lexer::compile(src).map_err(|e| format!("{name}: {e}"))?;
        let bytes = lx.to_bytes();
        assert_eq!(Lexer::from_bytes(&bytes)?, lx, "{name}");
        // deterministic build
        assert_eq!(Lexer::compile(src)?.to_bytes(), bytes, "{name}");
        assert_eq!(Lexer::bundled(name)?, lx);
    }
    assert!(matches!(Lexer::bundled("klingon"), Err(Error::UnknownLanguage(_))));
    assert!(Lexer::bundled("TR").is_ok());
    Ok(())
}

#[test]
fn analyses_in_each_language() -> TestResult {
    let cases: &[(&str, &str, &[&str], &str)] = &[
        ("english", "unhappiest", &["un<PFX:NEG>", "happy<ADJ>", "est<SUP>"], "un|happi|est"),
        ("turkish", "evlerde", &["ev<N>", "lAr<PL>", "DA<LOC>"], "ev|ler|de"),
        ("turkish", "saatler", &["saat<N>", "lAr<PL>"], "saat|ler"),
        ("spanish", "luces", &["luz<N>", "S<PL>"], "luc|es"),
        ("french", "chevaux", &["cheval<N>", "x<PL>"], "chevau|x"),
        ("igbo", "ịga", &["I<INF>", "ga<V:go>"], "ị|ga"),
        ("igbo", "iri", &["I<INF>", "ri<V:eat>"], "i|ri"),
        ("igbo", "gara", &["ga<V:go>", "rV<PST>"], "ga|ra"),
    ];
    for &(lang, w, tags, seg) in cases {
        let lx = Lexer::bundled(lang)?;
        let (got, s) = best(&lx, w).ok_or(format!("{lang} rejected {w}"))?;
        assert_eq!(got, tags, "{lang} {w}");
        assert_eq!(s, seg, "{lang} {w}");
    }
    // disharmonic forms are rejected
    for (lang, w) in [("turkish", "evlarda"), ("turkish", "saatlar"), ("igbo", "iga"), ("igbo", "ịri")] {
        assert!(best(&Lexer::bundled(lang)?, w).is_none(), "{lang} accepted {w}");
    }
    // Korean: Hangul in, jamo-level morphology, Hangul out for display
    let ko = Lexer::bundled("korean")?;
    let out = ko.lex_text("학교에서 먹었다", Mode::Strict);
    let toks: Vec<String> = out.iter().flat_map(|(_, o)| o.tokens()).collect();
    assert_eq!(toks, ["학교<N>", "에서<LOC.SRC>", "먹<V>", "EOᆻ<PST>", "다<DECL>"]);
    Ok(())
}

#[test]
fn fallback_is_lossless_and_hybrid_finds_affixes() -> TestResult {
    let lx = Lexer::bundled("english")?;
    let mut rng = Rng::new(11);
    let alpha: Vec<char> = "abcdefghijklmnopqrstuvwxyzéß".chars().collect();
    for _ in 0..2000 {
        let w: String = (0..1 + rng.below(14)).map(|_| *rng.pick(&alpha)).collect();
        for mode in [Mode::Strict, Mode::Hybrid] {
            match lx.lex(&w, mode) {
                WordOutput::Fallback(p) => assert_eq!(reconstruct(&p), w.as_bytes()),
                WordOutput::Analyzed { best, .. } => assert_eq!(best.iter().map(|m| &w[m.start..m.end]).collect::<String>(), w),
                WordOutput::Hybrid { segments, .. } => {
                    let mut s = String::new();
                    for seg in segments {
                        match seg {
                            Segment::Morph(m) => s.push_str(&w[m.start..m.end]),
                            Segment::Unknown { surface, pieces, .. } => {
                                assert_eq!(reconstruct(&pieces), surface.as_bytes());
                                s.push_str(&surface);
                            }
                        }
                    }
                    assert_eq!(s, w);
                }
            }
        }
    }
    match lx.lex("blorfing", Mode::Hybrid) {
        WordOutput::Hybrid { segments, unknown_chars } => {
            assert_eq!(unknown_chars, 5);
            assert!(matches!(&segments[0], Segment::Unknown { surface, guess, .. } if surface == "blorf" && guess == "V"));
            assert!(matches!(&segments[1], Segment::Morph(m) if m.tag == "PROG"));
        }
        other => panic!("{other:?}"),
    }
    // hybrid never changes a word the grammar accepts
    assert_eq!(lx.lex("unhappiest", Mode::Hybrid), lx.lex("unhappiest", Mode::Strict));
    Ok(())
}

#[test]
fn running_text_is_normalized_per_language() -> TestResult {
    let tr = Lexer::bundled("turkish")?;
    // Turkish casing: I → ı, İ → i
    assert_eq!(tr.normalize("IŞIK İzmir"), "ışık izmir");
    let words: Vec<String> = tr.lex_text("Evlerde, SAATLER!", Mode::Strict).into_iter().map(|(w, _)| w).collect();
    assert_eq!(words, ["evlerde", ",", "saatler", "!"]); // punctuation is kept
    // Igbo: tone marks are stripped, dot-below kept
    let ig = Lexer::bundled("igbo")?;
    assert_eq!(ig.normalize("Ị́ga"), "ịga");
    Ok(())
}

#[test]
fn exceptions_are_learned_from_raw_text() -> TestResult {
    let hand = Lexer::bundled("turkish")?;
    let spec = languages::TURKISH.replace(" @set:vow=fu", "") + "\nINDUCE vow IN Nouns Adjs\n";
    let corpus = include_str!("../data/turkish_corpus.txt");
    let (learned, ind) = Lexer::compile_with_corpus(&spec, corpus)?;
    let mut got: Vec<&str> = ind.exceptions().map(|d| d.surface.as_str()).collect();
    got.sort_unstable();
    assert_eq!(got, ["harf", "hayal", "kalp", "saat"]);
    assert!(ind.exceptions().all(|d| d.flag.as_deref() == Some("set:vow=fu") && !d.ambiguous));
    for w in ["saatler", "saatimiz", "harfleri", "kalbim", "hayaller", "evlerde", "kitaplarımızdan"] {
        assert_eq!(learned.lex(w, Mode::Strict), hand.lex(w, Mode::Strict), "{w}");
    }
    // the annotated spec compiles to the same lexer
    let annotated = ind.annotate(&spec);
    assert_eq!(annotated.matches("! learned").count(), 4);
    assert_eq!(Lexer::compile(&annotated)?.lex("saatler", Mode::Strict), hand.lex("saatler", Mode::Strict));
    // an unknown loanword: hybrid mode infers its exception
    match learned.lex("googlelar", Mode::Hybrid) {
        WordOutput::Hybrid { segments, .. } => {
            assert!(matches!(&segments[0], Segment::Unknown { guess, .. } if guess == "N vow=bu"));
        }
        other => panic!("{other:?}"),
    }
    Ok(())
}

#[test]
fn bad_input_is_an_error_not_a_panic() -> TestResult {
    match Lexer::compile("LEXICON Root\n  cat N Nowhere ;\n") {
        Err(Error::Spec { line, .. }) => assert!(line > 0),
        other => panic!("{other:?}"),
    }
    let bytes = Lexer::bundled("english")?.to_bytes();
    assert!(Lexer::from_bytes(&bytes[..bytes.len() / 2]).is_err());
    assert!(Lexer::from_bytes(b"not an artifact").is_err());
    let mut bad = bytes.clone();
    let k = bad.len() / 3;
    bad[k] ^= 0x55;
    assert!(Lexer::from_bytes(&bad).is_err());
    Ok(())
}
