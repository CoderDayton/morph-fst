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

use std::collections::HashMap;

use crate::morphotactics::Morphotactics;
use crate::output::WordOutput;
use crate::subword::WpTrie;

/// What to do with a word the grammar rejects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Full morphological parse, else a deterministic WordPiece split of
    /// the whole word ([`WordOutput::Fallback`]).
    #[default]
    Strict,
    /// A rejected word becomes known affixes around one unknown stem built
    /// from known stem endings ([`WordOutput::Hybrid`]): `blopped` →
    /// `⟨blopp→blop⟩<?V> + ed<PST>`. Accepted words: same as `Strict`.
    Hybrid,
}

/// The lexer, and the crate's single entry point: create one with
/// [`bundled`](Self::bundled), [`compile`](Self::compile),
/// [`compile_with_corpus`](Self::compile_with_corpus) or
/// [`from_bytes`](Self::from_bytes), then call [`lex`](Self::lex) on a word
/// or [`lex_text`](Self::lex_text) on running text.
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
pub struct Lexer {
    #[doc(hidden)]
    pub mt: Morphotactics,
    #[doc(hidden)]
    pub trie: WpTrie,
}

impl Lexer {
    /// Compile a bundled spec by name: `english`, `turkish`, `spanish`,
    /// `french`, `korean`, `igbo` (or `en`, `tr`, ...).
    pub fn bundled(name: &str) -> crate::Result<Self> {
        match crate::languages::by_name(name) {
            Some(src) => Self::compile(src),
            None => Err(crate::Error::UnknownLanguage(name.to_string())),
        }
    }

    /// Lex one word. The word must already be normalized for the language;
    /// [`Self::lex_text`] does that for you.
    pub fn lex(&self, word: &str, mode: Mode) -> WordOutput {
        match mode {
            Mode::Strict => self.lex_strict(word),
            Mode::Hybrid => self.lex_hybrid(word),
        }
    }

    /// Normalize `text` the way the spec's `OPTION`s say (case folding,
    /// Unicode composition, apostrophes, tone stripping, Hangul jamo), split
    /// it into words, and lex each one. Returns the normalized words with
    /// their outputs; spans are byte offsets into those words.
    pub fn lex_text(&self, text: &str, mode: Mode) -> Vec<(String, WordOutput)> {
        let norm = &self.mt.norm;
        let text = crate::normalize::normalize(text, norm);
        // Text repeats words; lex each distinct word once.
        let mut first: HashMap<&str, usize> = HashMap::new();
        let mut out: Vec<(String, WordOutput)> = Vec::new();
        for w in crate::pretok::split(&text, norm.apostrophe_in_words).into_iter().filter_map(|(s, e)| text.get(s..e)) {
            let cached = first.get(w).and_then(|&i| out.get(i)).map(|(_, o)| o.clone());
            let o = cached.unwrap_or_else(|| self.lex(w, mode));
            first.entry(w).or_insert(out.len());
            out.push((w.to_string(), o));
        }
        out
    }
}
