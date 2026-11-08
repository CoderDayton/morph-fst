//! `.morph` generation from UniMorph inflection tables: every input row
//! must come back as an analysis with its lemma and features, and forms
//! outside the tables must not be analyzed.

use morph_fst::generate::from_unimorph;
use morph_fst::normalize::{compose_hangul, normalize};
use morph_fst::rng::Rng;
use morph_fst::{Lexer, Mode, Segment, WordOutput};
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
    canon(s.split(';').map(str::to_string).collect())
}

/// A part of speech is dropped when a subtype of it (`V.CVB`) is there:
/// UniMorph writes a gerund both `V;V.CVB` and `V.CVB`.
fn canon(mut f: BTreeSet<String>) -> BTreeSet<String> {
    let bases: Vec<String> = f.iter().filter_map(|t| t.split_once('.').map(|(b, _)| b.to_string())).collect();
    for b in bases {
        f.remove(&b);
    }
    f
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
                    let got = canon(a.iter().flat_map(|m| m.tag.split(';').map(str::to_string)).collect());
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

const CITATION: &str = "\
cantar\tcantar\tV;NFIN
cantar\tcanto\tV;IND;PRS;1;SG
cantar\tcantando\tV.CVB;PRS
hablar\thablar\tV;NFIN
hablar\thablo\tV;IND;PRS;1;SG
hablar\thablando\tV.CVB;PRS
amar\tamar\tV;NFIN
amar\tamo\tV;IND;PRS;1;SG
amar\tamando\tV.CVB;PRS
";

/// An unknown stem takes the lemma ending its class gives every known stem
/// (`blorf` + `o` → `blorfar`), even when no known stem ends like it.
#[test]
fn unknown_stems_take_their_class_lemma_ending() -> TestResult {
    let g = from_unimorph(CITATION)?;
    let lx = Lexer::compile(&g.spec)?;
    let WordOutput::Hybrid { segments, .. } = lx.lex("blorfo", Mode::Hybrid) else { panic!("not hybrid") };
    let lemma = segments.iter().find_map(|s| if let Segment::Unknown { lemma, .. } = s { Some(lemma.as_str()) } else { None });
    assert_eq!(lemma, Some("blorfar"), "{segments:?}");
    Ok(())
}

/// Features of a reading, as [`canon`] compares them.
fn reading_feats(segs: &[Segment]) -> BTreeSet<String> {
    canon(
        segs.iter()
            .flat_map(|s| match s {
                Segment::Morph(m) => m.tag.split(';').map(str::to_string).collect::<Vec<_>>(),
                Segment::Unknown { guess, .. } => guess.split(';').map(str::to_string).collect(),
            })
            .filter(|t| !t.is_empty())
            .collect(),
    )
}

const ARABIC: &str = "\
كتب\tكَتَبَ\tV;3;SG;MASC;PST
كتب\tكَتَبَتْ\tV;3;SG;FEM;PST
درس\tدَرَسَ\tV;3;SG;MASC;PST
درس\tدَرَسَتْ\tV;3;SG;FEM;PST
";

/// Vowel marks (Arabic harakat, Hebrew niqqud) are optional in writing and
/// absent from most text and from UniMorph's lemmas: a table with marked
/// forms gives a lexer that ignores them, with the plain lemma.
#[test]
fn vowel_marks_are_stripped_when_the_tables_use_them() -> TestResult {
    let g = from_unimorph(ARABIC)?;
    assert!(g.spec.contains("OPTION strip_vowel_marks true"), "{}", g.spec);
    let lx = Lexer::compile(&g.spec)?;
    for w in ["كتبت", "كَتَبَتْ"] {
        match lx.lex(&normalize(w, &lx.mt.norm), Mode::Strict) {
            WordOutput::Analyzed { best, .. } => assert_eq!(best.first().map(|m| m.lemma.as_str()), Some("كتب"), "{w}"),
            o => panic!("{w}: {}", o.render()),
        }
    }
    let he = morph_fst::normalize::NormOptions { strip_vowel_marks: true, ..Default::default() };
    assert_eq!(normalize("שָׁלוֹם", &he), "שלום");
    Ok(())
}

/// Lemma of the unknown stem in the readings of `w` that have features
/// `want`, best first.
fn unknown_lemmas(lx: &Lexer, w: &str, want: &[&str]) -> Vec<String> {
    let want: BTreeSet<String> = want.iter().map(|s| s.to_string()).collect();
    let WordOutput::Hybrid { all, .. } = lx.lex(&normalize(w, &lx.mt.norm), Mode::Hybrid) else { return Vec::new() };
    all.iter()
        .filter(|r| reading_feats(r) == want)
        .filter_map(|r| r.iter().find_map(|s| if let Segment::Unknown { lemma, .. } = s { Some(lemma.clone()) } else { None }))
        .collect()
}

const GERMAN: &str = "\
machen\tmachen\tV;NFIN
machen\tmacht\tV;IND;PRS;3;SG
machen\tgemacht\tV.PTCP;PST
sagen\tsagen\tV;NFIN
sagen\tsagt\tV;IND;PRS;3;SG
sagen\tgesagt\tV.PTCP;PST
legen\tlegen\tV;NFIN
legen\tlegt\tV;IND;PRS;3;SG
legen\tgelegt\tV.PTCP;PST
";

const HEBREW: &str = "\
הגדיל\tהגדיל\tV;PST;3;SG;MASC
הגדיל\tמגדיל\tV;PRS;SG;MASC
הלביש\tהלביש\tV;PST;3;SG;MASC
הלביש\tמלביש\tV;PRS;SG;MASC
הרגיש\tהרגיש\tV;PST;3;SG;MASC
הרגיש\tמרגיש\tV;PRS;SG;MASC
";

const SPANISH_CLITICS: &str = "\
hablar\thablar\tV;NFIN
hablar\thablo\tV;IND;PRS;1;SG
hablar\tháblala\tV;IMP;INFM;2;SG;ACC;PRO;3;SG
cantar\tcantar\tV;NFIN
cantar\tcanto\tV;IND;PRS;1;SG
cantar\tcántala\tV;IMP;INFM;2;SG;ACC;PRO;3;SG
";

/// German nouns: lemmas are capitalized, forms are read lowercased.
const GERMAN_NOUNS: &str = "\
Kindheit\tKindheit\tN;SG
Kindheit\tKindheiten\tN;PL
Freiheit\tFreiheit\tN;SG
Freiheit\tFreiheiten\tN;PL
Wahrheit\tWahrheit\tN;SG
Wahrheit\tWahrheiten\tN;PL
";

/// Hebrew present tense; a lemma ends in a final letter (`ך ם ן ף ץ`).
const HEBREW_FINALS: &str = "\
הדריך\tהדריך\tV;PST;3;SG;MASC
הדריך\tמדריכים\tV;PRS;PL;MASC
הדריך\tמדריכה\tV;PRS;SG;FEM
הסמיך\tהסמיך\tV;PST;3;SG;MASC
הסמיך\tמסמיכים\tV;PRS;PL;MASC
הסמיך\tמסמיכה\tV;PRS;SG;FEM
";

/// A stem's start can change too: a prefix (German `ge-`), a swapped
/// prefix (Hebrew present `מ-` for past `ה-`) or one marked letter
/// (Spanish `á` before clitics). Unseen stems borrow these from known ones.
#[test]
fn unknown_stems_borrow_changes_at_the_start() -> TestResult {
    for (tsv, word, feats, lemma) in [
        (GERMAN, "gekauft", &["V.PTCP", "PST"][..], "kaufen"),
        (HEBREW, "מסביר", &["V", "PRS", "SG", "MASC"][..], "הסביר"),
        (SPANISH_CLITICS, "trángala", &["V", "IMP", "INFM", "2", "SG", "ACC", "PRO", "3"][..], "trangar"),
        (GERMAN_NOUNS, "torheiten", &["N", "PL"][..], "Torheit"),
        (HEBREW_FINALS, "מגליפים", &["V", "PRS", "PL", "MASC"][..], "הגליף"),
        (HEBREW_FINALS, "מגליפה", &["V", "PRS", "SG", "FEM"][..], "הגליף"),
    ] {
        let g = from_unimorph(tsv)?;
        let lx = Lexer::compile(&g.spec)?;
        assert_round_trip(tsv, &lx);
        let got = unknown_lemmas(&lx, word, feats);
        assert_eq!(got.first().map(String::as_str), Some(lemma), "{word}: {got:?}\n{}", g.spec);
    }
    Ok(())
}

/// Participles, converbs and verbal nouns (`V.CVB`) belong to the verb's
/// paradigm, so an unseen verb's gerund reads like its other forms.
#[test]
fn verb_subtypes_join_the_verb_paradigm() -> TestResult {
    let g = from_unimorph(CITATION)?;
    let lx = Lexer::compile(&g.spec)?;
    let WordOutput::Hybrid { all, .. } = lx.lex("blorfando", Mode::Hybrid) else { panic!("not hybrid") };
    let want: BTreeSet<String> = ["V.CVB", "PRS"].iter().map(|s| s.to_string()).collect();
    let hit = all.iter().any(|r| {
        let lemma = r.iter().find_map(|s| if let Segment::Unknown { lemma, .. } = s { Some(lemma.as_str()) } else { None });
        lemma == Some("blorfar") && reading_feats(r) == want
    });
    assert!(hit, "{all:?}");
    Ok(())
}

/// `V.CVB;V` and `V;V.CVB` are the same cell, whichever comes first.
#[test]
fn subtype_order_does_not_split_cells() -> TestResult {
    let tsv = "hablar\thablar\tV;NFIN\nhablar\thablando\tV.CVB;V;PRS\nhablar\thablo\tV;IND;PRS;1;SG\n\
               comer\tcomer\tV;NFIN\ncomer\tcomiendo\tV;V.CVB;PRS\ncomer\tcomo\tV;IND;PRS;1;SG\n";
    let g = from_unimorph(tsv)?;
    let lx = Lexer::compile(&g.spec)?;
    let tags = |w: &str| match lx.lex(w, Mode::Strict) {
        WordOutput::Analyzed { best, .. } => best.iter().map(|m| m.tag.clone()).collect::<Vec<_>>().join("+"),
        other => format!("{other:?}"),
    };
    assert_eq!(tags("hablando"), "V;V.CVB;PRS");
    assert_eq!(tags("comiendo"), "V;V.CVB;PRS");
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
