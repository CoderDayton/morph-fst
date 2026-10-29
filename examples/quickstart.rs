//! Quickstart: get a lexer, analyze text, save it.
//!
//!     cargo run --example quickstart

use morph_fst::{Kind, Lexer};

fn main() -> morph_fst::Result<()> {
    let lx = Lexer::bundled("turkish")?;

    // 2. Analyze running text. It is normalized (Turkish casing: I → ı)
    //    and split into words first.
    for word in lx.analyze_text("Kitaplarımızdan bloglarımda") {
        let how = match word.kind() {
            Kind::Known => "known",
            Kind::Guessed => "unknown stem, known endings",
            _ => "split into pieces",
        };
        println!("{}: {how}", word.text());
        for part in word.parts() {
            let mark = if part.is_known() { "" } else { " (guessed)" };
            println!("    {:<8} lemma={:<8} tag={}{mark}", part.text(), part.lemma(), part.tag());
        }
    }

    println!("tokens: {:?}", lx.analyze("evlerde").tokens());

    // 4. Save the compiled lexer as one file; loading skips compilation.
    let path = std::env::temp_dir().join("turkish.mcfu");
    lx.save(&path)?;
    assert_eq!(Lexer::load(&path)?, lx);
    println!("saved to {}", path.display());
    Ok(())
}
