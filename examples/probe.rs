//! usage: probe <spec.morph> word...
use morph_fst::{Lexer, Mode};
use morph_fst::normalize::{compose_hangul, normalize};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: probe <spec.morph> word...")?;
    let lx = Lexer::compile(&std::fs::read_to_string(&path)?)?;
    for w in args {
        let w = normalize(&w, &lx.mt.norm);
        println!("{:>16}  S: {}", compose_hangul(&w), lx.lex(&w, Mode::Strict).render());
        let h = lx.lex(&w, Mode::Hybrid);
        if h != lx.lex(&w, Mode::Strict) {
            println!("{:>16}  H: {}", "", h.render());
        }
    }
    Ok(())
}
