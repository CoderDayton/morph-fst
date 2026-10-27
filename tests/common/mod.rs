#![allow(dead_code)]
use morph_fst::Lexer;
use morph_fst::spec::{self, Grammar};

pub const SPEC: &str = include_str!("../../data/english.morph");

pub type TestResult = Result<(), Box<dyn std::error::Error>>;

pub fn grammar() -> morph_fst::Result<Grammar> {
    spec::load(SPEC)
}

pub fn fused() -> morph_fst::Result<Lexer> {
    Lexer::compile(SPEC)
}

/// `spec` with every `@flag`ged stem of its open lexicons but only every
/// `k`-th other one, small enough for the exhaustive oracle.
pub fn sample(spec: &str, k: usize) -> String {
    let (mut open, mut i) = (false, 0usize);
    let mut out = String::new();
    for line in spec.lines() {
        let t = line.trim();
        if t.starts_with("LEXICON ") || t == "RULES" || t == "PIECES" {
            open = t.contains("@open");
        } else if open && !t.is_empty() && !t.starts_with('!') && !t.contains(" @") {
            i += 1;
            if i % k != 0 {
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}
