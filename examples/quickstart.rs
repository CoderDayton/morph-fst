//! Quickstart: compile a bundled language, lex some text, save the artifact.
//!
//!     cargo run --example quickstart

use morph_fst::{Lexer, Mode, Segment, WordOutput};

fn main() -> morph_fst::Result<()> {
    // 1. Compile a bundled spec (or `Lexer::compile(&your_spec_text)`).
    let lx = Lexer::bundled("turkish")?;

    // 2. Lex running text. `lex_text` normalizes (Turkish casing: I → ı)
    //    and splits words; each word gets one of three outputs.
    for (word, out) in lx.lex_text("Kitaplarımızdan bloglarımda xqzv", Mode::Hybrid) {
        match &out {
            // Known word: a full morphological analysis.
            WordOutput::Analyzed { best, all } => {
                println!("{word}: analysed ({} reading(s))", all.len());
                for m in best {
                    println!("    {:<8} lemma={:<8} tag={}", m.surface, m.lemma, m.tag);
                }
            }
            // Hybrid mode: known affixes around one unknown stem.
            WordOutput::Hybrid { segments, .. } => {
                println!("{word}: unknown stem + known affixes");
                for s in segments {
                    match s {
                        Segment::Morph(m) => println!("    {:<8} tag={}", m.surface, m.tag),
                        Segment::Unknown { surface, guess, .. } => println!("    {surface:<8} guess={guess}"),
                    }
                }
            }
            // Nothing fits: deterministic WordPiece split (never fails).
            WordOutput::Fallback(_) => println!("{word}: subword fallback {:?}", out.tokens()),
        }
    }

    // 3. Flat tokens for a downstream model.
    let toks: Vec<String> = lx.lex_text("evlerde", Mode::Strict).into_iter().flat_map(|(_, o)| o.tokens()).collect();
    println!("tokens: {toks:?}");

    // 4. Ship the compiled lexer as one file; loading skips compilation.
    let bytes = lx.to_bytes();
    let again = Lexer::from_bytes(&bytes)?;
    assert_eq!(again, lx);
    println!("artifact: {} bytes", bytes.len());
    Ok(())
}
