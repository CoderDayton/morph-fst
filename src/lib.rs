//! # morph-fst
//!
//! A rule-based **morphological analyzer** for agglutinative and inflecting
//! languages, fused with a **deterministic subword fallback** for words the
//! grammar does not know. One spec file in, one compiled artifact out, one
//! left-to-right scan per word. No dependencies by default.
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
//! # Ok::<(), morph_fst::Error>(())
//! ```
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
//! * [`WordOutput::Hybrid`] (with [`Mode::Hybrid`]): known prefixes and
//!   suffixes around one unknown stem of an open class, e.g. Turkish
//!   `bloglarımda` → `⟨blog⟩<?N> + lAr<PL> + Ixm<P1SG> + DA<LOC>`.
//!
//! ## Languages
//!
//! Each language is a plain-text `.morph` spec. The spec language is
//! lexc/twolc-like: lexicons with continuation classes, sequential
//! rewrite rules, archiphonemes, and context features. [`languages`] bundles
//! compact demo specs for English, Turkish, Spanish, French, Korean and Igbo.
//! Compile your own with [`Lexer::compile`]. The spec language covers:
//!
//! * **long-distance harmony in both directions**: `FEATURE` (left context,
//!   e.g. Turkish suffix harmony) and `RFEATURE` (right context, e.g. Igbo
//!   prefixes agreeing with the root);
//! * **opaque (non-surface-true) harmony**: `OPTION harmony underlying`
//!   makes harmony read each morpheme's underlying form (or an explicit
//!   `@tier=`), not the surface after rules;
//! * **lexical exceptions**: marked by hand (`@set:vow=fu` on a root),
//!   or **learned from raw text**. Declare `INDUCE vow IN Nouns` and call
//!   [`Lexer::compile_with_corpus`] (see [`induce`]).
//!
//! ## Saving and loading
//!
//! [`Lexer::to_bytes`] / [`Lexer::from_bytes`] round-trip the compiled
//! lexer. Loading validates every table, so a corrupt file is an error,
//! never a panic.
//!
//! ## Cargo features
//!
//! * `cli`: the `morph-fst` command-line tool.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unreachable, clippy::todo, clippy::unimplemented)]

pub mod error;
pub mod fused;
pub mod induce;
pub mod languages;
pub mod morphotactics;
pub mod normalize;
pub mod oracle;
pub mod output;
pub mod spec;
pub mod subword;

#[doc(hidden)]
pub mod codec;
#[doc(hidden)]
pub mod lattice;
#[doc(hidden)]
pub mod pretok;
#[doc(hidden)]
pub mod rng;

pub use error::{Error, Result};
pub use fused::{FusedLexer, FusedLexer as Lexer, Mode};
pub use induce::{Decision, Induction};
pub use output::{Analysis, Morph, Segment, SubToken, WordOutput};

/// Maximum analyses kept per word in [`WordOutput::Analyzed::all`].
pub const ANALYSES_CAP: usize = 16;

/// Anything that lexes a single, already-normalized word.
pub trait WordLexer {
    /// Lex one normalized word.
    fn lex_word(&self, word: &str) -> WordOutput;
    /// The language's normalization settings (from its spec).
    fn norm(&self) -> normalize::NormOptions {
        normalize::NormOptions::default()
    }
}

/// Normalize `text` for the lexer's language, pre-tokenize, lex each word.
/// Returned words (and all spans) refer to the normalized text.
pub fn lex_text<L: WordLexer + ?Sized>(lx: &L, text: &str) -> Vec<(String, WordOutput)> {
    lex_text_with(lx, text, &lx.norm())
}

/// Like [`lex_text`] with explicit normalization options.
pub fn lex_text_with<L: WordLexer + ?Sized>(lx: &L, text: &str, norm: &normalize::NormOptions) -> Vec<(String, WordOutput)> {
    let text = normalize::normalize(text, norm);
    pretok::split(&text, norm.apostrophe_in_words)
        .into_iter()
        .map(|(s, e)| {
            let w = &text[s..e];
            (w.to_string(), lx.lex_word(w))
        })
        .collect()
}
