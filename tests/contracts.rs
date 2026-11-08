//! Guarantees at the edges: what a loaded artifact may spell, which
//! readings come back, and that adding or saving a grammar keeps its meaning.

use morph_fst::codec::{self, Writer};
use morph_fst::{Kind, Lexer, Mode, WordOutput};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn loader_rejects_pieces_that_do_not_spell_their_path() -> TestResult {
    let lx = Lexer::compile("LEXICON Root\n a N # ;\n b N # ;\n")?;
    let mut mw = Writer::new();
    lx.mt.write(&mut mw);
    let mut t = codec::unwrap(b"MCFU", &lx.to_bytes())?.to_vec();
    let u32at = |b: &[u8], at: usize| b.get(at..at + 4).and_then(|s| s.try_into().ok()).map_or(0, u32::from_le_bytes) as usize;
    // After the morphotactics: nine u32 arrays, then the pieces: flag, length, text.
    let mut at = mw.into_inner().len();
    for _ in 0..9 {
        at += 4 + 4 * u32at(&t, at);
    }
    let np = u32at(&t, at);
    at += 4;
    let mut changed = false;
    for _ in 0..np {
        at += 1;
        let len = u32at(&t, at);
        at += 4;
        if t.get(at..at + len) == Some(b"a".as_slice()) {
            t[at] = b'b';
            changed = true;
        }
        at += len;
    }
    assert!(changed, "the trie should hold the piece `a`");
    assert!(Lexer::from_bytes(&codec::wrap(b"MCFU", &t)).is_err());
    Ok(())
}

#[test]
fn saving_keeps_the_mode() -> TestResult {
    let strict = Lexer::compile("LEXICON Root\n 0 Stems ;\nLEXICON Stems @open=V\n walk V Sfx ;\n talk V Sfx ;\n jump V Sfx ;\nLEXICON Sfx\n 0 INF # ;\n ing PROG # ;\n")?.mode(Mode::Strict);
    let back = Lexer::from_bytes(&strict.to_bytes())?;
    assert_eq!(back.analyze("blorfing").kind(), strict.analyze("blorfing").kind());
    assert_eq!(strict.analyze("blorfing").kind(), Kind::Unsplit);
    let hybrid = Lexer::from_bytes(&strict.mode(Mode::Hybrid).to_bytes())?;
    assert_eq!(hybrid.analyze("blorfing").kind(), Kind::Guessed);
    Ok(())
}

#[test]
fn alternatives_are_the_best_ones() -> TestResult {
    let many: String = (0..513).map(|i| format!(" c T{i} # ;\n")).collect();
    let spec = format!(
        "LEXICON Root\n 0 A ;\n 0 B ;\n 0 C ;\nLEXICON A\n a FIRST A1 ;\nLEXICON A1\n b MIDDLE A2 ;\nLEXICON A2\n{many}LEXICON B\n ab SECOND Tail ;\nLEXICON Tail\n c END # ;\nLEXICON C\n abc BEST # ;\n"
    );
    let WordOutput::Analyzed { all, .. } = Lexer::compile(&spec)?.lex("abc", Mode::Strict) else { panic!("abc should parse") };
    let lengths: Vec<usize> = all.iter().take(3).map(Vec::len).collect();
    assert_eq!(lengths, [1, 2, 3]);
    Ok(())
}

#[test]
fn repeated_entries_change_nothing() -> TestResult {
    for n in [1, 16] {
        let spec = format!("PREFER V AFTER PRON\nLEXICON Root\n they PRON # ;\n{} run V # ;\n", " run N # ;\n".repeat(n));
        let words = Lexer::compile(&spec)?.analyze_text("they run");
        assert_eq!(words[1].parts()[0].tag(), "V", "{n} noun entries");
        assert_eq!(words[1].alternatives().len(), 1, "{n} noun entries");
    }
    let shapes = |n: usize| -> Result<String, morph_fst::Error> {
        let spec = format!("LEXICON Root\n 0 Nouns ;\nLEXICON Nouns @open=N\n{}LEXICON S\n s PL # ;\nRULES\n y_ie: y -> ie / C _ +{{s}} ;\n", " city N S ;\n".repeat(n));
        Ok(Lexer::compile(&spec)?.analyze("blorties").lemma().to_string())
    };
    assert_eq!(shapes(2)?, shapes(1)?);
    Ok(())
}

#[test]
fn a_name_shows_one_reading_everywhere() -> TestResult {
    let en = Lexer::bundled("english")?;
    let words = en.analyze_text("visit Blorfing");
    let w = &words[1];
    assert_eq!(w.kind(), Kind::Name);
    assert_eq!(w.tokens(), [w.text()]);
    assert_eq!(w.to_string(), w.text());
    Ok(())
}

#[test]
fn extending_a_spec_keeps_its_meaning() -> TestResult {
    for src in ["! LEXICON Root\nLEXICON Root\n cat N # ;\n", "LEXICON\tRoot\n cat N # ;\n"] {
        let lx = Lexer::builder().spec(src).words("zorp").build()?;
        assert_eq!(lx.analyze("zorp").kind(), Kind::Known, "{src:?}");
        assert_eq!(lx.analyze("cat").kind(), Kind::Known, "{src:?}");
    }
    let lx = Lexer::builder().spec("OPTION case none\nLEXICON Root\n cat N # ;\n").words("Zorp").build()?;
    assert_eq!(lx.analyze("Zorp").kind(), Kind::Known);
    Ok(())
}
