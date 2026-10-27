//! "Never crash": hostile text, hostile artifacts, hostile specs.

mod common;
use common::TestResult;
use morph_fst::{Lexer, Mode};
use morph_fst::output::{reconstruct, Segment, WordOutput};
use morph_fst::rng::Rng;
use morph_fst::spec;

fn surface_bytes(out: &WordOutput) -> Vec<u8> {
    match out {
        WordOutput::Analyzed { best, .. } => best.iter().flat_map(|m| m.surface.bytes()).collect(),
        WordOutput::Fallback(p) => reconstruct(p),
        WordOutput::Hybrid { segments, .. } => segments
            .iter()
            .flat_map(|s| match s {
                Segment::Morph(m) => m.surface.clone().into_bytes(),
                Segment::Unknown { pieces, .. } => reconstruct(pieces),
            })
            .collect(),
    }
}

#[test]
fn random_unicode_is_total_and_lossless() -> TestResult {
    let f = common::fused()?;
    let mut rng = Rng::new(9);
    for _ in 0..20_000 {
        let w: String = (0..rng.below(12))
            .filter_map(|_| {
                let r = rng.next_u64();
                let cp = match r % 4 {
                    0 => (r >> 8) as u32 % 0x80,
                    1 => 0x61 + (r >> 8) as u32 % 26,
                    2 => (r >> 8) as u32 % 0x3000,
                    _ => (r >> 8) as u32 % 0x11_0000,
                };
                char::from_u32(cp)
            })
            .collect();
        for mode in [Mode::Strict, Mode::Hybrid] {
            let out = f.lex(&w, mode);
            assert_eq!(surface_bytes(&out), w.as_bytes(), "{w:?}");
        }
    }
    Ok(())
}

#[test]
fn megabyte_word() -> TestResult {
    let f = common::fused()?;
    let w = "unhappiness".repeat(100_000); // > max_morph_chars: straight to fallback
    let mut steps = 0;
    let p = f.trie.tokenize_counted(&w, &mut steps);
    assert!(steps <= 2 * w.len() as u64);
    assert_eq!(reconstruct(&p), w.as_bytes());
    assert_eq!(f.lex(&w, Mode::Strict), WordOutput::Fallback(p));
    let emoji = "😀".repeat(250_000);
    assert_eq!(surface_bytes(&f.lex(&emoji, Mode::Strict)), emoji.as_bytes());
    Ok(())
}

#[test]
fn builds_are_byte_deterministic() -> TestResult {
    assert_eq!(Lexer::compile(common::SPEC)?.to_bytes(), Lexer::compile(common::SPEC)?.to_bytes());
    Ok(())
}

#[test]
fn artifacts_round_trip() -> TestResult {
    let f = common::fused()?;
    let f2 = Lexer::from_bytes(&f.to_bytes())?;
    assert_eq!(f, f2);
    for w in ["unhappiness", "unhappyness", "walks", "😀", ""] {
        assert_eq!(f.lex(w, Mode::Strict), f2.lex(w, Mode::Strict));
    }
    Ok(())
}

#[test]
fn corrupt_artifacts_are_errors_not_panics() -> TestResult {
    let mut rng = Rng::new(5);
    let data = common::fused()?.to_bytes();
    for _ in 0..900 {
        let cut = rng.below(data.len());
        let mut flipped = data.to_vec();
        let k = rng.below(flipped.len());
        flipped[k] ^= 1 << rng.below(8);
        for bad in [&data[..cut], &flipped[..]] {
            assert!(Lexer::from_bytes(bad).is_err());
        }
    }
    assert!(Lexer::from_bytes(&[]).is_err());
    let mut foreign = data.clone();
    foreign[..4].copy_from_slice(b"XXXX");
    assert!(Lexer::from_bytes(&foreign).is_err()); // wrong magic
    Ok(())
}

#[test]
fn oracle_rejects_invalid_specs() -> morph_fst::Result<()> {
    // Parses, but does not compile: the rule names an undeclared CLASS.
    let bad = spec::parse_spec("LEXICON Root\n x N S ;\nLEXICON S\n s P # ;\nRULES\n r: x -> y / [Nope] _ +{s} ;")?;
    assert!(morph_fst::oracle::generate(&bad, 4, 1000).is_err());
    Ok(())
}

#[test]
fn bad_specs_are_errors() {
    let cases = [
        ("", "missing `LEXICON Root`"),
        ("LEXICON Root\n 0 A ;\nLEXICON A\n 0 Root ;", "epsilon cycle"),
        ("LEXICON Root\n cat N Nope ;", "unknown continuation"),
        ("LEXICON Root\n cat N #", "must end with"),
        ("LEXICON Root\n 0 X Y # ;", "zero morpheme is"),
        ("LEXICON Root\n :x N # ;", "non-epsilon entry"),
        ("LEXICON Root\n cat # ;", "two-field"),
        ("LEXICON Root\n x{A} N S ;\nLEXICON S\n s P # ;\nRULES\n r1: x -> y / _ [*] +{s} ;\nFEATURE f\n a -> b\nARCHI A\n * -> a", "before an archiphoneme"),
        ("LEXICON Root\n x{Q} N # ;", "unknown archiphoneme"),
        ("LEXICON Root\n x N # ;\nRULES\n r: x -> y / [Nope] _ +{s} ;", "unknown CLASS"),
        ("FEATURE f\n a -> b\nARCHI A\n f=b -> x\nLEXICON Root\n x N # ;", "no realisation"),
        ("FEATURE f\n a -> b\nARCHI A\n * -> 0\nLEXICON Root\n {A} N # ;", "empty string"),
        ("LEXICON Root\n x N # @set:nope=v ;", "unknown FEATURE"),
        ("LEXICON Root\n x N S ;\nLEXICON S\n s P # ;\nRULES\n r1: x -> 0 / _ +{s} ;", "deletes all"),
        ("LEXICON Root\nLEXICON Root", "duplicate"),
        ("OPTION max_morph_chars 0", "max_morph_chars"),
        ("RULES\n r: y -> i / % _ +{a} ;", "unknown left context"),
        ("cat N # ;", "before any"),
    ];
    for (src, needle) in cases {
        match spec::load(src) {
            Err(e) => assert!(e.to_string().contains(needle), "{src:?}: got `{e}`, want `{needle}`"),
            Ok(_) => panic!("{src:?} should fail"),
        }
    }
}
