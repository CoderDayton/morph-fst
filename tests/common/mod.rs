#![allow(dead_code)]
use morph_fst::fused::FusedLexer;
use morph_fst::spec::{self, Grammar};

pub const SPEC: &str = include_str!("../../data/english.morph");

pub type TestResult = Result<(), Box<dyn std::error::Error>>;

pub fn grammar() -> morph_fst::Result<Grammar> {
    spec::load(SPEC)
}

pub fn fused() -> morph_fst::Result<FusedLexer> {
    FusedLexer::compile(SPEC)
}
