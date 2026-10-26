//! Dump strict+hybrid outputs for a spec over a corpus (fingerprinting).
//! usage: outdump <spec.morph> <corpus.txt>
use morph_fst::fused::{FusedLexer, Mode};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let (Some(spec), Some(corpus)) = (a.get(1), a.get(2)) else {
        return Err("usage: outdump <spec.morph> <corpus.txt>".into());
    };
    let lx = FusedLexer::compile(&std::fs::read_to_string(spec)?)?;
    for w in std::fs::read_to_string(corpus)?.lines() {
        println!("{:?}\t{:?}", lx.lex(w, Mode::Strict), lx.lex(w, Mode::Hybrid));
    }
    Ok(())
}
