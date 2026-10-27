//! Rule oracle: the compiled analyzer must accept exactly
//! the surfaces an independent string-level generator produces (up to 4
//! morphemes), with exactly the same glosses — and must reject naive
//! rule-less concatenations that the rules forbid.

mod common;
use common::TestResult;
use morph_fst::oracle::{self, Gloss};
use morph_fst::output::WordOutput;
use morph_fst::spec::parse_spec;
use std::collections::BTreeSet;

const DEPTH: usize = 4;

fn glosses(out: Option<WordOutput>) -> BTreeSet<Gloss> {
    match out {
        Some(WordOutput::Analyzed { all, .. }) => all
            .into_iter()
            .filter(|a| a.len() <= DEPTH)
            .map(|a| a.into_iter().map(|m| (m.lemma, m.tag)).collect())
            .collect(),
        _ => BTreeSet::new(),
    }
}

#[test]
fn analyzers_match_string_level_oracle() -> TestResult {
    // The full lexicon exceeds the oracle's path budget; every rule and flag survives the sample.
    let text = common::sample(common::SPEC, 16);
    let spec = parse_spec(&text)?;
    let gen = oracle::generate(&spec, DEPTH, 5_000_000)?;
    assert!(gen.surfaces.len() > 10_000, "oracle too small: {}", gen.surfaces.len());
    let f = morph_fst::Lexer::compile(&text)?;
    for (surface, want) in &gen.surfaces {
        let got_f = glosses(f.analyze(surface, 512));
        assert_eq!(&got_f, want, "fused: {surface}");
    }
    let mut rejected = 0;
    for naive in &gen.naive {
        if gen.surfaces.contains_key(naive) {
            continue;
        }
        assert!(glosses(f.analyze(naive, 512)).is_empty(), "fused accepted forbidden {naive}");
        rejected += 1;
    }
    assert!(rejected > 1000, "too few negative cases: {rejected}");
    eprintln!("oracle: {} surfaces checked, {} forbidden forms rejected", gen.surfaces.len(), rejected);
    Ok(())
}
