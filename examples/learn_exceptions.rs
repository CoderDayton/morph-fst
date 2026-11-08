//! Learn lexical harmony exceptions from raw text instead of marking them
//! by hand.
//!
//! Turkish loanwords like *saat* 'hour' have back vowels but take front
//! suffixes (saat-ler, not *saat-lar). The small demo Turkish grammar marks
//! them by hand (`@set:vow=fu`). Here we delete those marks, declare where
//! exceptions may occur (`INDUCE vow IN Nouns`), and learn them back from a
//! few sentences of ordinary text.
//!
//! cargo run --example learn_exceptions
use morph_fst::{Lexer, Mode};

const TURKISH: &str = include_str!("../tests/fixtures/turkish.morph");

fn main() -> morph_fst::Result<()> {
    let spec = TURKISH.replace(" @set:vow=fu", "") + "\nINDUCE vow IN Nouns Adjs\n";
    let corpus = include_str!("../data/turkish_corpus.txt");
    let (lexer, learned) = Lexer::compile_with_corpus(&spec, corpus)?;
    println!("corpus: {} tokens, {} types", learned.tokens, learned.types);
    for d in learned.exceptions() {
        println!("learned  {:<8} @{}   (support {} vs surface {}{})", d.surface, d.flag.as_deref().unwrap_or(""), d.support, d.surface_support, if d.ambiguous { ", ambiguous" } else { "" });
    }
    for w in ["saatler", "saatimiz", "harfleri", "kalbim", "hayaller"] {
        println!("{w:>10}  {}", lexer.lex(w, Mode::Strict).render());
    }
    // Unknown words: hybrid mode infers the exception on the fly.
    for w in ["googlelar", "googlelerden", "blogumuz"] {
        println!("{w:>10}  {}", lexer.lex(w, Mode::Hybrid).render());
    }
    Ok(())
}
