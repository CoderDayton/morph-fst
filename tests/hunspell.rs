//! Hunspell dictionaries feed the generator.

use morph_fst::hunspell::{decode, to_unimorph};
use morph_fst::{Kind, Lexer};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const AFF: &str = "\
SET UTF-8
TRY esianrtolcdugmphbyfvkwz
PFX U Y 1
PFX U   0     un         .
SFX S Y 3
SFX S   y     ies        [^aeiou]y
SFX S   0     s          [aeiou]y
SFX S   0     s          [^y]
SFX D Y 2
SFX D   0     ed/N       [^e]
SFX D   0     d          e
SFX N N 1
SFX N   0     ness       .
FORBIDDENWORD X
NEEDAFFIX Z
";

const DIC: &str = "\
7
city/S
party/S
play/SDU
walk/SD	po:noun
like/SDU
bake/D
colour/X
";

#[test]
fn affix_rules_expand_with_their_conditions() -> TestResult {
    let rows = to_unimorph(AFF, DIC)?;
    for want in ["city\tcities\tSTEM;SFX_S", "play\tplays\tSTEM;SFX_S", "like\tliked\tSTEM;SFX_D", "play\tunplay\tSTEM;PFX_U", "play\tunplays\tSTEM;PFX_U;SFX_S", "play\tplayedness\tSTEM;SFX_D;SFX_N", "walk\twalks\tN;SFX_S"] {
        assert!(rows.lines().any(|r| r == want), "{want} missing from\n{rows}");
    }
    assert!(!rows.contains("colour"), "forbidden word kept");
    assert!(!rows.contains("citys"), "a condition was ignored");
    Ok(())
}

#[test]
fn a_dictionary_builds_a_lexer_that_guesses() -> TestResult {
    let lx = Lexer::builder().hunspell(AFF, DIC).build()?;
    assert_eq!(lx.analyze("parties").kind(), Kind::Known);
    let w = lx.analyze("ponies");
    assert_eq!((w.kind(), w.lemma()), (Kind::Guessed, "pony"), "{w:?}");
    Ok(())
}

#[test]
fn flags_come_long_numbered_or_aliased() -> TestResult {
    let long = "FLAG long\nSFX Aa Y 1\nSFX Aa 0 s .\n";
    assert!(to_unimorph(long, "cat/Aa\n")?.contains("cat\tcats\tSTEM;SFX_Aa"));
    let num = "FLAG num\nSFX 12 Y 1\nSFX 12 0 s .\n";
    assert!(to_unimorph(num, "cat/3,12\n")?.contains("cat\tcats\tSTEM;SFX_12"));
    let alias = "AF 1\nAF S\nSFX S Y 1\nSFX S 0 s .\n";
    assert!(to_unimorph(alias, "cat/1\n")?.contains("cat\tcats\tSTEM;SFX_S"));
    assert!(to_unimorph("SFX S Y 1\nSFX S 0 s [ab\n", "cat/S\n").is_err());
    Ok(())
}

#[test]
fn aliases_reach_continuations_and_boms_are_skipped() -> TestResult {
    let aff = "AF 2\nAF N\nAF D\nSFX D Y 1\nSFX D 0 ed/1 .\nSFX N Y 1\nSFX N 0 ness .\n";
    assert!(to_unimorph(aff, "\u{FEFF}1\nplay/2\n")?.contains("play\tplayedness\tSTEM;SFX_D;SFX_N"));
    assert_eq!(decode(b"\xEF\xBB\xBFcaf\xC3\xA9", b"\xEF\xBB\xBFSET UTF-8\n")?, "café");
    Ok(())
}

#[test]
fn a_rule_keeps_a_stem_letter() -> TestResult {
    assert!(!to_unimorph("SFX S Y 1\nSFX S y ies y\n", "y/S\n")?.contains("ies"));
    Ok(())
}

#[test]
fn encodings_follow_set() -> TestResult {
    assert_eq!(decode(b"caf\xe9", b"SET ISO8859-1\n")?, "café");
    assert_eq!(decode(b"\xa4", b"SET ISO8859-15\n")?, "€");
    assert!(decode(b"x", b"SET KOI8-R\n").is_err());
    Ok(())
}
