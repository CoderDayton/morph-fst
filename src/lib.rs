//! # morph-fst
//!
//! A rule-based **morphological analyzer** fused with a **deterministic
//! subword fallback** for words the grammar does not know. One spec file in,
//! one compiled artifact out, one left-to-right scan per word. No
//! dependencies by default.
//!
//! [`Lexer`] is the single entry point: build it, then lex.
//!
//! ```
//! use morph_fst::{Lexer, Mode, WordOutput};
//!
//! let lx = Lexer::bundled("english")?;
//!
//! // A known word: full analysis (lemma + tag per morpheme).
//! let out = lx.lex("unhappiest", Mode::Strict);
//! assert_eq!(out.tokens(), ["un<PFX:NEG>", "happy<ADJ>", "est<SUP>"]);
//!
//! // An unknown word: deterministic WordPiece split, never a failure.
//! assert!(matches!(lx.lex("blorfq", Mode::Strict), WordOutput::Fallback(_)));
//!
//! // Hybrid mode: known affixes around one unknown stem.
//! assert!(matches!(lx.lex("blorfing", Mode::Hybrid), WordOutput::Hybrid { .. }));
//!
//! // Running text: normalized for the language, split into words.
//! let words = lx.lex_text("The Unhappiest children", Mode::Hybrid);
//! assert_eq!(words.len(), 3);
//! # Ok::<(), morph_fst::Error>(())
//! ```
//!
//! ## Building a lexer
//!
//! * [`Lexer::bundled`]: a bundled spec by name (English, Turkish, Spanish,
//!   French, Korean, Igbo).
//! * [`Lexer::compile`]: your own `.morph` spec text.
//! * [`Lexer::compile_with_corpus`]: the same, after learning which roots
//!   are lexical exceptions from raw text (`INDUCE` in the spec).
//! * [`Lexer::from_bytes`]: a lexer saved with [`Lexer::to_bytes`]. Loading
//!   validates every table, so a corrupt file is an error, never a panic.
//!
//! [`generate::from_unimorph`] drafts a `.morph` spec from UniMorph tables.
//!
//! ## What a word becomes
//!
//! [`Lexer::lex`] returns a [`WordOutput`]:
//!
//! * [`WordOutput::Analyzed`]: the grammar accepts the word. You get the best
//!   analysis and every alternative (morphemes with byte spans, surface,
//!   lemma, tag).
//! * [`WordOutput::Fallback`]: the grammar rejects it, so it is split by
//!   WordPiece longest-match (LinMaxMatch, linear time). The vocabulary is
//!   derived from the grammar itself, and characters outside it fall back to
//!   bytes. Concatenating the pieces always gives back the word.
//! * [`WordOutput::Hybrid`] (with [`Mode::Hybrid`]): known affixes around one
//!   unknown stem whose class, spelling and lemma come from known stems that
//!   end the same way, every reading ranked.
//!
//! ## Cargo features
//!
//! * `cli`: the `morph-fst` command-line tool.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unreachable, clippy::todo, clippy::unimplemented)]

pub mod generate;

// Internals: public for the CLI, tests and examples, not part of the API.
#[doc(hidden)]
pub mod codec;
#[doc(hidden)]
pub mod error;
#[doc(hidden)]
pub mod fused;
#[doc(hidden)]
pub mod induce;
#[doc(hidden)]
pub mod languages;
#[doc(hidden)]
pub mod lattice;
#[doc(hidden)]
pub mod morphotactics;
#[doc(hidden)]
pub mod normalize;
#[doc(hidden)]
pub mod oracle;
#[doc(hidden)]
pub mod output;
#[doc(hidden)]
pub mod pretok;
#[doc(hidden)]
pub mod rng;
#[doc(hidden)]
pub mod shapes;
#[doc(hidden)]
pub mod spec;
#[doc(hidden)]
pub mod subword;

pub use error::{Error, Result};
pub use fused::{Lexer, Mode};
pub use induce::{Decision, Induction};
pub use output::{Analysis, Morph, Segment, SubToken, WordOutput};

/// Maximum analyses kept per word in [`WordOutput::Analyzed::all`].
pub const ANALYSES_CAP: usize = 16;
