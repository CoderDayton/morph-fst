//! The fused lexer — **one source, one compile loop, one artifact**.
//!
//! The compiler builds a single char trie in which
//!
//! * a node may be a WordPiece token (word-initial or `##`),
//! * a node may carry morpheme payloads (entry ids), and
//! * every node gets a LinMaxMatch failure link + failure pops,
//!
//! all produced by one insertion loop over a unified, sorted key stream and
//! one BFS. The piece vocabulary is *derived* from the morphotactics (which
//! lexicons can start a word, which can follow a morpheme), so the two layers
//! cannot drift apart.
//!
//! Why a trie and not a minimal FST? A minimal FST shares suffixes, so one
//! node has many prefixes and an Aho–Corasick failure link ("longest proper
//! suffix that is a prefix") is undefined. Failure links need a trie.

mod artifact;
mod builder;
mod hybrid;
mod scan;

pub use builder::derive_vocab;

use crate::morphotactics::Morphotactics;
use crate::output::WordOutput;
use crate::subword::WpTrie;
use crate::WordLexer;

/// What to do with a word the grammar rejects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Full morphological parse, else a deterministic WordPiece split of
    /// the whole word ([`WordOutput::Fallback`]).
    #[default]
    Strict,
    /// Like `Strict`, but a rejected word is first tried as known affixes
    /// around **one** unknown stem of an open class
    /// ([`WordOutput::Hybrid`]), e.g. Turkish `bloglarımda` →
    /// `⟨blog⟩<?N> + lAr<PL> + Ixm<P1SG> + DA<LOC>`. Identical to `Strict` on every word the
    /// grammar accepts.
    Hybrid,
}

/// The lexer: one compiled artifact holding the morphological transducer
/// and the subword fallback (exported at the crate root as [`crate::Lexer`]).
///
/// ```
/// use morph_fst::{Lexer, Mode, WordOutput};
///
/// let lx = Lexer::bundled("turkish")?;
/// let out = lx.lex("evlerde", Mode::Strict);
/// assert!(matches!(out, WordOutput::Analyzed { .. }));
/// if let WordOutput::Analyzed { best, .. } = out {
///     let tags: Vec<&str> = best.iter().map(|m| m.tag.as_str()).collect();
///     assert_eq!(tags, ["N", "PL", "LOC"]);
/// }
/// # Ok::<(), morph_fst::Error>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FusedLexer {
    /// Morphotactics, context registers and morpheme entries (low level).
    pub mt: Morphotactics,
    /// The shared character trie: morpheme payloads + WordPiece pieces.
    pub trie: WpTrie,
}

impl FusedLexer {
    /// Compile one of the specs bundled in [`crate::languages`] by name
    /// (`"english"`, `"turkish"`, `"tr"`, ...).
    pub fn bundled(name: &str) -> crate::Result<Self> {
        match crate::languages::by_name(name) {
            Some(src) => Self::compile(src),
            None => Err(crate::Error::UnknownLanguage(name.to_string())),
        }
    }

    /// Lex one word. The word must already be normalized for the language
    /// (see [`Self::normalize`]); [`Self::lex_text`] does that for you.
    pub fn lex(&self, word: &str, mode: Mode) -> WordOutput {
        match mode {
            Mode::Strict => self.lex_strict(word),
            Mode::Hybrid => self.lex_hybrid(word),
        }
    }

    /// Normalize running text for this language (case folding, Unicode
    /// composition, apostrophes, tone stripping, Hangul jamo, as the spec's
    /// `OPTION`s say).
    pub fn normalize(&self, text: &str) -> String {
        crate::normalize::normalize(text, &self.mt.norm)
    }

    /// Normalize `text`, split it into words, and lex each one. Returns the
    /// normalized words with their outputs; spans in the outputs are byte
    /// offsets into those words.
    pub fn lex_text(&self, text: &str, mode: Mode) -> Vec<(String, WordOutput)> {
        crate::lex_text(&self.with_mode(mode), text)
    }

    /// A [`WordLexer`] view running in `mode`.
    pub fn with_mode(&self, mode: Mode) -> ModeLexer<'_> {
        ModeLexer { lx: self, mode }
    }
}

impl WordLexer for FusedLexer {
    fn lex_word(&self, word: &str) -> WordOutput {
        self.lex_strict(word)
    }
    fn norm(&self) -> crate::normalize::NormOptions {
        self.mt.norm
    }
}

/// A [`FusedLexer`] fixed to one [`Mode`]; see [`FusedLexer::with_mode`].
pub struct ModeLexer<'a> {
    lx: &'a FusedLexer,
    mode: Mode,
}

impl WordLexer for ModeLexer<'_> {
    fn lex_word(&self, word: &str) -> WordOutput {
        self.lx.lex(word, self.mode)
    }
    fn norm(&self) -> crate::normalize::NormOptions {
        self.lx.mt.norm
    }
}
