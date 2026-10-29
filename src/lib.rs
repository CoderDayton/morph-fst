//! # morph-fst
//!
//! A rule-based **morphological analyzer** fused with a **deterministic
//! subword fallback** for words the grammar does not know. One spec file in,
//! one compiled artifact out, one left-to-right scan per word.
//!
//! Three steps: get a [`Lexer`], [`analyze`](Lexer::analyze) words, read
//! the [`Word`].
//!
//! ```
//! use morph_fst::{Kind, Lexer};
//!
//! let lx = Lexer::bundled("english")?;
//!
//! // A word the grammar knows: every part has a lemma and a tag.
//! let word = lx.analyze("unhappiest");
//! assert_eq!(word.kind(), Kind::Known);
//! assert_eq!(word.tokens(), ["un<PFX:NEG>", "happy<ADJ>", "est<SUP>"]);
//!
//! // A word it has never seen: known endings around a guessed stem.
//! let word = lx.analyze("blorfing");
//! assert_eq!(word.kind(), Kind::Guessed);
//! assert_eq!(word.parts()[0].text(), "blorf");
//! assert_eq!(word.parts()[1].tag(), "PROG");
//!
//! // Running text is normalized and split into words for you.
//! assert_eq!(lx.analyze_text("The Unhappiest children").len(), 3);
//! # Ok::<(), morph_fst::Error>(())
//! ```
//!
//! ## Getting a lexer
//!
//! * [`Lexer::bundled`]: ready-made (English, Turkish, Spanish, French,
//!   Korean, Igbo).
//! * [`Lexer::builder`]: generate a new language from whatever data exists
//!   for it: UniMorph tables, a word list, a hand-written spec, a corpus.
//!   See [`LexerBuilder`].
//! * [`Lexer::save`] and [`Lexer::load`]: compile once, reuse the file.
//!
//! ## What a word becomes
//!
//! [`Lexer::analyze`] returns a [`Word`] whose [`kind`](Word::kind) says how
//! far the grammar got:
//!
//! * [`Kind::Known`]: the grammar accepts the word.
//! * [`Kind::Guessed`]: known endings around one unknown stem, whose class,
//!   spelling and lemma come from known stems that end the same way.
//! * [`Kind::Unsplit`]: no reading, so the word is cut into subword pieces
//!   (only with [`Mode::Strict`]; characters outside the grammar become
//!   bytes, and the pieces always join back into the word).
//!
//! [`Word::detail`] gives the full [`WordOutput`] (byte spans, every reading).
//!
//! ## Cargo features
//!
//! * `cli`: the `morph-fst` command-line tool.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unreachable, clippy::todo, clippy::unimplemented)]

pub mod discover;
pub mod generate;

mod build;
mod word;

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

pub use build::LexerBuilder;
pub use error::{Error, Result};
pub use fused::{Lexer, Mode};
pub use induce::{Decision, Induction};
pub use output::{Analysis, Morph, Segment, SubToken, WordOutput};
pub use word::{Kind, Part, Word};

/// Maximum analyses kept per word in [`WordOutput::Analyzed::all`].
pub const ANALYSES_CAP: usize = 16;
