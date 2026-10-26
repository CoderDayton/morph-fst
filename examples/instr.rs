//! One pass over a corpus in a given mode, for instruction counting:
//! valgrind --tool=callgrind ... instr <corpus> <none|build|strict|hybrid>
use morph_fst::fused::{FusedLexer, Mode};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let (Some(corpus), Some(mode)) = (a.get(1), a.get(2)) else {
        return Err("usage: instr <corpus> <none|build|strict|hybrid>".into());
    };
    let words: Vec<String> = std::fs::read_to_string(corpus)?.lines().map(String::from).collect();
    let owned = std::env::var("SPEC").ok().map(std::fs::read_to_string).transpose()?;
    let src: &str = owned.as_deref().unwrap_or(include_str!("../data/english.morph"));
    let f = FusedLexer::compile(src)?;
    let mut n = 0usize;
    if mode == "build" {
        for _ in 0..50 {
            n += FusedLexer::compile(src)?.to_bytes().len();
        }
    }
    for w in &words {
        n += match mode.as_str() {
            "strict" => std::hint::black_box(f.lex(w, Mode::Strict)).source().len(),
            "hybrid" => std::hint::black_box(f.lex(w, Mode::Hybrid)).source().len(),
            _ => 0,
        };
    }
    println!("{n}");
    Ok(())
}
