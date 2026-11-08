//! `PREFER` rules: a word's best reading in running text follows its
//! neighbours, as the grammar orders.

use morph_fst::Lexer;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SPEC: &str = "\
PREFER V AFTER PRON,\"to\"
PREFER ADJ OVER ADV BEFORE N
PREFER N AFTER DET

LEXICON Root
  they  PRON # ;
  to    PREP # ;
  the   DET  # ;
  run   N    # ;
  run   V    # ;
  fast  ADV  # ;
  fast  ADJ  # ;
  car   N    # ;
";

fn tags(lx: &Lexer, text: &str) -> Vec<String> {
    lx.analyze_text(text).iter().map(|w| w.parts().first().map(|p| p.tag().to_string()).unwrap_or_default()).collect()
}

#[test]
fn neighbours_pick_the_reading() -> TestResult {
    let lx = Lexer::compile(SPEC)?;
    // Alone, a word keeps the grammar's order.
    assert_eq!(lx.analyze("run").parts()[0].tag(), "N");
    assert_eq!(lx.analyze("fast").parts()[0].tag(), "ADV");
    assert_eq!(tags(&lx, "they run"), ["PRON", "V"]);
    assert_eq!(tags(&lx, "to run"), ["PREP", "V"]);
    assert_eq!(tags(&lx, "the run"), ["DET", "N"]);
    assert_eq!(tags(&lx, "the fast car"), ["DET", "ADJ", "N"]);
    // `OVER`: only an ADV reading gives way, and only before a noun.
    assert_eq!(tags(&lx, "they run fast"), ["PRON", "V", "ADV"]);
    // The old best stays first among the alternatives.
    let w = &lx.analyze_text("they run")[1];
    assert_eq!(w.alternatives()[0][0].tag(), "N");
    Ok(())
}

/// `tokens()` and `Display` show the reading the context chose.
#[test]
fn detail_follows_the_chosen_reading() -> TestResult {
    let lx = Lexer::compile(SPEC)?;
    let w = &lx.analyze_text("they run")[1];
    assert_eq!(w.tokens(), ["run<V>"]);
    assert!(w.to_string().starts_with("run<V>"), "{w}");
    Ok(())
}

/// An entry spelled `PREFER` is still an entry, and so is a piece.
#[test]
fn prefer_is_spellable() -> TestResult {
    let lx = Lexer::compile("OPTION case none\nLEXICON Root\n  PREFER N # ;\nPIECES\n  PREFER\n")?;
    assert_eq!(lx.analyze("PREFER").parts()[0].tag(), "N");
    Ok(())
}

#[test]
fn bad_prefer_lines_are_errors() {
    for line in ["PREFER V", "PREFER V NEAR N", "PREFER V OVER AFTER N", "PREFER \"\" AFTER N", "PREFER V AFTER ,"] {
        let spec = format!("{line}\nLEXICON Root\n  a N # ;\n");
        assert!(Lexer::compile(&spec).is_err(), "{line} should be an error");
    }
}

#[test]
fn rules_survive_save_and_load() -> TestResult {
    let lx = Lexer::from_bytes(&Lexer::compile(SPEC)?.to_bytes())?;
    assert_eq!(tags(&lx, "they run"), ["PRON", "V"]);
    Ok(())
}
