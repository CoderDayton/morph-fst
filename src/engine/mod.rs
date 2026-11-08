//! The engine: a `.morph` spec compiled into one lexer that splits words.
//!
//! `spec` parses and compiles the grammar, `morphotactics` holds which
//! morpheme may follow which, `subword` is the shared trie, `fused` scans a
//! word with both, `lattice` ranks the readings, `shapes` lets hybrid mode
//! guess unknown stems, and `codec` saves and loads the result.

pub mod codec;
pub mod fused;
pub mod lattice;
pub mod morphotactics;
pub mod oracle;
pub mod shapes;
pub mod spec;
pub mod subword;
