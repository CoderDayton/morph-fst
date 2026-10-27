//! `.morph` generation from UniMorph inflection tables: every input row
//! must come back as an analysis with its lemma and features, and forms
//! outside the tables must not be analyzed.

use morph_fst::generate::from_unimorph;
use morph_fst::normalize::{compose_hangul, normalize};
use morph_fst::rng::Rng;
use morph_fst::{Lexer, Mode, WordOutput};
use std::collections::BTreeSet;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const ENGLISH: &str = "\
walk\twalk\tV;NFIN
walk\twalks\tV;PRS;3;SG
walk\twalked\tV;PST
walk\twalking\tV;V.PTCP;PRS
talk\ttalk\tV;NFIN
talk\ttalks\tV;PRS;3;SG
talk\ttalked\tV;PST
talk\ttalking\tV;V.PTCP;PRS
go\tgo\tV;NFIN
go\tgoes\tV;PRS;3;SG
go\twent\tV;PST
go\tgoing\tV;V.PTCP;PRS
cat\tcat\tN;SG
cat\tcats\tN;PL
box\tbox\tN;SG
box\tboxes\tN;PL
";

fn feats(s: &str) -> BTreeSet<String> {
    s.split(';').map(str::to_string).collect()
}

/// Every row is analyzed, with one reading carrying its lemma and features.
fn assert_round_trip(tsv: &str, lx: &Lexer) {
    for row in tsv.lines().filter(|l| !l.trim().is_empty()) {
        let cols: Vec<&str> = row.split('\t').collect();
        let (lemma, form, want) = (cols[0], cols[1], feats(cols[2]));
        let w = normalize(form, &lx.mt.norm);
        match lx.lex(&w, Mode::Strict) {
            WordOutput::Analyzed { all, .. } => {
                let hit = all.iter().any(|a| {
                    let got: BTreeSet<String> = a.iter().flat_map(|m| m.tag.split(';').map(str::to_string)).collect();
                    a.first().is_some_and(|m| compose_hangul(&m.lemma) == lemma) && got == want
                });
                assert!(hit, "{form}: no reading {lemma} {want:?} among {all:?}");
            }
            o => panic!("{form} not analyzed: {}", o.render()),
        }
    }
}

#[test]
fn english_tables_round_trip() -> TestResult {
    let g = from_unimorph(ENGLISH)?;
    let lx = Lexer::compile(&g.spec)?;
    assert_round_trip(ENGLISH, &lx);
    for w in ["walkes", "wents", "boxs", "catses", "goed"] {
        assert!(matches!(lx.lex(w, Mode::Strict), WordOutput::Fallback(_)), "{w} should fall back");
    }
    // walk and talk share one class; go, cat and box each have their own.
    assert_eq!((g.lemmas, g.forms, g.paradigms, g.irregular), (5, 16, 4, 1));
    assert!(g.skipped.is_empty());
    // regular forms split at the stem
    match lx.lex("walked", Mode::Strict) {
        WordOutput::Analyzed { best, .. } => assert_eq!(morph_fst::output::segmentation(&best), "walk|ed"),
        o => panic!("{}", o.render()),
    }
    Ok(())
}

const SPANISH: &str = "\
orar\torar\tV;NFIN
orar\toro\tV;IND;PRS;1;SG
orar\toran\tV;IND;PRS;3;PL
orar\toramos\tV;IND;PRS;1;PL
orar\tórala\tV;IMP;INFM;2;SG;ACC;PRO;3;SG
hablar\thablar\tV;NFIN
hablar\thablo\tV;IND;PRS;1;SG
hablar\thablan\tV;IND;PRS;3;PL
hablar\thablamos\tV;IND;PRS;1;PL
hablar\tháblala\tV;IMP;INFM;2;SG;ACC;PRO;3;SG
pensar\tpensar\tV;NFIN
pensar\tpienso\tV;IND;PRS;1;SG
pensar\tpiensan\tV;IND;PRS;3;PL
pensar\tpensamos\tV;IND;PRS;1;PL
";

fn best_segmentation(lx: &Lexer, w: &str) -> String {
    match lx.lex(w, Mode::Strict) {
        WordOutput::Analyzed { best, .. } => morph_fst::output::segmentation(&best),
        o => o.render(),
    }
}

/// Accent shifts split at the stem, and stem changes split before a
/// suffix other lemmas use in the same cell: nothing is listed whole.
#[test]
fn stem_variants_split_instead_of_listing_whole() -> TestResult {
    let g = from_unimorph(SPANISH)?;
    let lx = Lexer::compile(&g.spec)?;
    assert_round_trip(SPANISH, &lx);
    assert_eq!(g.irregular, 0, "{}", g.spec);
    assert_eq!(best_segmentation(&lx, "órala"), "ór|ala");
    assert_eq!(best_segmentation(&lx, "piensan"), "piens|an");
    assert_eq!(best_segmentation(&lx, "pensamos"), "pens|amos");
    Ok(())
}

#[test]
fn unusable_rows_are_skipped_and_reported() -> TestResult {
    let tsv = "walk\twalk\tV;NFIN\nbroken line\nwalk\twalk ed\tV;PST\nwalk\tw!x\tV;PST\nwalk\twalks\tV;PRS;3;SG\n";
    let g = from_unimorph(tsv)?;
    assert_eq!(g.forms, 2);
    let lines: Vec<usize> = g.skipped.iter().map(|(l, _)| *l).collect();
    assert_eq!(lines, [2, 3, 4]);
    assert!(from_unimorph("").is_err());
    assert!(from_unimorph("only\ttwo\n").is_err());
    Ok(())
}

/// The lemma is written into the spec too, so it cannot hold reserved
/// characters or spaces either; the row is skipped, not the whole file.
#[test]
fn reserved_lemmas_are_skipped_not_fatal() -> TestResult {
    for lemma in ["a:b", "a{b", "a#b", "a;b", "ice cream"] {
        let tsv = format!("walk\twalk\tV;NFIN\n{lemma}\tab\tN;SG\nwalk\twalks\tV;PRS;3;SG\n");
        let g = from_unimorph(&tsv)?;
        assert_eq!(g.forms, 2, "{lemma}");
        let lines: Vec<usize> = g.skipped.iter().map(|(l, _)| *l).collect();
        assert_eq!(lines, [2], "{lemma}");
    }
    Ok(())
}

/// `0` alone means "empty" in a spec, so a stem or suffix that is exactly
/// `0` cannot be written; such forms are listed whole.
#[test]
fn zero_stems_and_suffixes_are_listed_whole() -> TestResult {
    let tsv = "0\t0\tN;SG\n0\t0s\tN;PL\n10\t10\tN;SG\n10\t100\tN;PL\n1\t1\tN;SG\n1\t1s\tN;PL\nn00b\tn00b\tN;SG\nn00b\tn00bs\tN;PL\n";
    let g = from_unimorph(tsv)?;
    let lx = Lexer::compile(&g.spec)?;
    assert_round_trip(&tsv.replace("0\t0\tN;SG\n", ""), &lx);
    Ok(())
}

#[test]
fn input_order_does_not_matter() -> TestResult {
    let mut rows: Vec<&str> = ENGLISH.lines().collect();
    rows.reverse();
    assert_eq!(from_unimorph(&rows.join("\n"))?.spec, from_unimorph(ENGLISH)?.spec);
    Ok(())
}

#[test]
fn case_hangul_and_apostrophes_follow_the_lexer() -> TestResult {
    let tsv = "Haus\tHaus\tN;NOM;SG\nHaus\tHäuser\tN;NOM;PL\nHaus\tHauses\tN;GEN;SG\n\
               먹다\t먹어요\tV;PRS;POL\n먹다\t먹었어요\tV;PST;POL\n먹다\t먹다\tV;NFIN\n\
               aujourd'hui\taujourd'hui\tADV\n";
    let g = from_unimorph(tsv)?;
    let lx = Lexer::compile(&g.spec)?;
    assert_round_trip(tsv, &lx);
    Ok(())
}

/// Random paradigms with stem changes, syncretism, bare forms and
/// suppletive forms: every row must round-trip.
#[test]
fn random_tables_round_trip() -> TestResult {
    let mut rng = Rng::new(9);
    let alpha: Vec<char> = "abdegiklmnoprstuzäöüı0".chars().collect();
    let word = |rng: &mut Rng, lo: usize, hi: usize| -> String { (0..lo + rng.below(hi - lo)).map(|_| *rng.pick(&alpha)).collect() };
    for round in 0..40 {
        let pos = *rng.pick(&["N", "V", "ADJ"]);
        let cells: Vec<String> = (0..2 + rng.below(8)).map(|i| format!("{pos};F{}", i % 5)).collect();
        let suffix_sets: Vec<Vec<String>> = (0..1 + rng.below(4))
            .map(|_| cells.iter().map(|_| if rng.chance(1, 6) { String::new() } else { word(&mut rng, 1, 4) }).collect())
            .collect();
        let mut tsv = String::new();
        let mut seen = BTreeSet::new();
        for _ in 0..3 + rng.below(30) {
            let stem = word(&mut rng, 2, 7);
            if !seen.insert(stem.clone()) {
                continue;
            }
            let lemma = stem.clone() + "x";
            let sfx = rng.pick(&suffix_sets).clone();
            for (cell, s) in cells.iter().zip(&sfx) {
                let form = if rng.chance(1, 12) { word(&mut rng, 2, 8) } else { format!("{stem}{s}") };
                tsv.push_str(&format!("{lemma}\t{form}\t{cell}\n"));
            }
        }
        let g = from_unimorph(&tsv).map_err(|e| format!("round {round}: {e}"))?;
        let lx = Lexer::compile(&g.spec).map_err(|e| format!("round {round}: {e}\n{}", g.spec))?;
        assert_round_trip(&tsv, &lx);
    }
    Ok(())
}
