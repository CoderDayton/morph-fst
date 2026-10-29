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
    pub(crate) mode: Mode,
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

    /// Start describing a language to generate: tables, word lists, a spec,
    /// a corpus, in any combination. See [`LexerBuilder`](crate::LexerBuilder).
    pub fn builder() -> crate::LexerBuilder {
        crate::LexerBuilder::default()
    }

    /// What to do with words the grammar does not know. New lexers guess
    /// ([`Mode::Hybrid`]); pass [`Mode::Strict`] to split them into subword
    /// pieces instead.
    pub fn mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }

    /// Analyze one word (normalized for the language first).
    pub fn analyze(&self, word: &str) -> crate::Word {
        let text = crate::normalize::normalize(word, &self.mt.norm);
        let detail = self.lex(&text, self.mode);
        crate::Word::new(text, 0..word.len(), detail, &crate::word::Policy::of(&self.mt))
    }

    /// Analyze running text: [`tokenize`](crate::tokenize) it, then analyze
    /// each word. Numbers, URLs, emails, abbreviations, clitics, punctuation
    /// and symbols come back as [`Kind::Literal`](crate::Kind::Literal). Each
    /// [`Word::span`](crate::Word::span) points into `text`. Long texts use
    /// every core; the result is the same.
    pub fn analyze_text(&self, text: &str) -> Vec<crate::Word> {
        const PARALLEL_FROM: usize = 4096;
        let threads = if text.len() < PARALLEL_FROM { 1 } else { 0 };
        let tokens = crate::tokenize::scan(text, self.mt.norm.apostrophe_in_words);
        let policy = crate::word::Policy::of(&self.mt);
        let normalized: Vec<Option<String>> = tokens
            .iter()
            .map(|t| {
                let surface = text.get(t.span()).unwrap_or_default();
                (t.class() == crate::Class::Word).then(|| crate::normalize::normalize(surface, &self.mt.norm)).filter(|n| !n.is_empty())
            })
            .collect();
        let lexed: Vec<&str> = normalized.iter().flatten().map(String::as_str).collect();
        let mut outs = self.lex_words(&lexed, self.mode, threads).into_iter();
        tokens
            .iter()
            .zip(normalized)
            .enumerate()
            .map(|(i, (t, norm))| {
                let surface = text.get(t.span()).unwrap_or_default();
                let lexed = match norm {
                    Some(n) => outs.next().map(|detail| crate::Word::new(n, t.span(), detail, &policy)),
                    None => None,
                };
                match lexed {
                    Some(w) if w.kind() == crate::Kind::Guessed && crate::tokenize::is_capitalized(surface) && !crate::tokenize::starts_sentence(text, &tokens, i) => w.into_name(),
                    Some(w) => w,
                    None => crate::Word::literal(&surface.replace('\u{2019}', "'"), t.class(), t.span()),
                }
            })
            .collect()
    }

    /// Search terms for `text`: one lowercase key per word, its lemma, so a
    /// query and a document meet however the word is inflected (`ran`,
    /// `running` → `run`). Numbers, emails, URLs and abbreviations stay
    /// whole; punctuation, symbols and clitics are dropped. Use it on both
    /// the documents and the queries.
    pub fn terms(&self, text: &str) -> Vec<String> {
        const KEPT: [&str; 4] = ["NUM", "URL", "EMAIL", "ABBR"];
        self.analyze_text(text)
            .iter()
            .filter(|w| w.kind() != crate::Kind::Literal || w.parts().first().is_some_and(|p| KEPT.contains(&p.tag())))
            .map(|w| crate::normalize::normalize(w.lemma(), &self.mt.norm))
            .filter(|t| !t.is_empty())
            .collect()
    }

    /// Save the compiled lexer to a file; [`load`](Self::load) reads it back
    /// without recompiling.
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> crate::Result<()> {
        Ok(std::fs::write(path, self.to_bytes())?)
    }

    /// Load a lexer saved with [`save`](Self::save). A damaged or foreign
    /// file is an error.
    pub fn load(path: impl AsRef<std::path::Path>) -> crate::Result<Self> {
        Self::from_bytes(&std::fs::read(path)?)
    }

    /// Lex one word with an explicit mode. The word must already be
    /// normalized; most callers want [`analyze`](Self::analyze).
    #[doc(hidden)]
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
    #[doc(hidden)]
    pub fn lex_text(&self, text: &str, mode: Mode) -> Vec<(String, WordOutput)> {
        self.lex_text_par(text, mode, 1)
    }

    /// [`lex_text`](Self::lex_text) with the work spread over `threads`
    /// threads (`0` = one per core). The output is identical. Each distinct
    /// word is lexed once, and repeats get a clone of its output.
    #[doc(hidden)]
    pub fn lex_text_par(&self, text: &str, mode: Mode, threads: usize) -> Vec<(String, WordOutput)> {
        let norm = &self.mt.norm;
        let text = crate::normalize::normalize(text, norm);
        let words: Vec<&str> = crate::pretok::split(&text, norm.apostrophe_in_words).into_iter().filter_map(|(s, e)| text.get(s..e)).collect();
        let outs = self.lex_words(&words, mode, threads);
        words.iter().zip(outs).map(|(w, o)| (w.to_string(), o)).collect()
    }

    /// Lex each of `words` (already normalized): distinct words once, over
    /// `threads` threads (`0` = one per core). Output order matches input.
    fn lex_words(&self, words: &[&str], mode: Mode, threads: usize) -> Vec<WordOutput> {
        let mut index: HashMap<&str, usize> = HashMap::new();
        let mut distinct: Vec<&str> = Vec::new();
        let slots: Vec<usize> = words
            .iter()
            .map(|&w| {
                *index.entry(w).or_insert_with(|| {
                    distinct.push(w);
                    distinct.len() - 1
                })
            })
            .collect();
        let threads = match threads {
            0 => std::thread::available_parallelism().map_or(1, std::num::NonZero::get),
            n => n,
        }
        .min(distinct.len().max(1));
        let lex_all = |ws: &[&str]| -> Vec<WordOutput> { ws.iter().map(|w| self.lex(w, mode)).collect() };
        let outs: Vec<WordOutput> = if threads <= 1 {
            lex_all(&distinct)
        } else {
            std::thread::scope(|s| {
                let chunks = distinct.chunks(distinct.len().div_ceil(threads));
                let handles: Vec<_> = chunks.clone().map(|c| s.spawn(move || lex_all(c))).collect();
                chunks.zip(handles).flat_map(|(c, h)| h.join().unwrap_or_else(|_| lex_all(c))).collect()
            })
        };
        words.iter().zip(slots).map(|(&w, i)| outs.get(i).cloned().unwrap_or_else(|| self.lex(w, mode))).collect()
    }

    /// A lexer drafted straight from [UniMorph](https://unimorph.github.io)
    /// inflection tables (`lemma<TAB>form<TAB>features`), for any of its
    /// languages: [`generate::from_unimorph`](crate::generate::from_unimorph)
    /// then [`compile`](Self::compile), with no spec to write.
    #[doc(hidden)]
    pub fn from_unimorph(tsv: &str) -> crate::Result<Self> {
        Self::compile(&crate::generate::from_unimorph(tsv)?.spec)
    }

    /// A lexer drafted from a plain word list, with no labels at all:
    /// [`discover::from_wordlist`](crate::discover::from_wordlist) then
    /// [`compile`](Self::compile).
    #[doc(hidden)]
    pub fn from_wordlist(text: &str) -> crate::Result<Self> {
        Self::compile(&crate::discover::from_wordlist(text)?.spec)
    }
}
