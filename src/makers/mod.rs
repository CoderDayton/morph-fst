//! Grammar makers: write a `.morph` spec from data instead of by hand.
//!
//! `generate` reads UniMorph tables, `hunspell` spell-checker dictionaries,
//! `discover` a plain word list, and `induce` learns exceptions from raw
//! text. `build` is [`LexerBuilder`](crate::LexerBuilder), which combines them.

pub(crate) mod build;
pub mod discover;
pub mod generate;
pub mod hunspell;
pub mod induce;
