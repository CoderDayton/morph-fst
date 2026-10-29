//! [`LexerBuilder`]: generate a language from whatever data exists for it.

use crate::error::{Error, Result};
use crate::fused::{Lexer, Mode};
use crate::output::WordOutput;

/// Describes a language to generate. Give it any of these, then
/// [`build`](Self::build):
///
/// * [`unimorph`](Self::unimorph): inflection tables with features and
///   lemmas (the richest source);
/// * [`spec`](Self::spec): a hand-written `.morph` grammar, instead of tables;
/// * [`words`](Self::words): a plain word list. On its own it yields stems and
///   endings with no features; next to tables or a spec it covers the words
///   they miss;
/// * [`corpus`](Self::corpus): raw text, used to learn which roots are
///   exceptions (a spec's `INDUCE` lines).
///
/// ```
/// use morph_fst::{Kind, Lexer};
///
/// let tables = "walk\twalk\tV;NFIN\nwalk\twalked\tV;PST\ntalk\ttalk\tV;NFIN\ntalk\ttalked\tV;PST\n";
/// let lx = Lexer::builder().unimorph(tables).words("kick kicked kicks kicking").build()?;
/// assert_eq!(lx.analyze("kicked").kind(), Kind::Known);
/// # Ok::<(), morph_fst::Error>(())
/// ```
#[derive(Clone, Debug, Default)]
pub struct LexerBuilder {
    spec: Option<String>,
    unimorph: Option<String>,
    words: Option<String>,
    corpus: Option<String>,
}

impl LexerBuilder {
    /// A `.morph` grammar written by hand.
    pub fn spec(mut self, spec: &str) -> Self {
        self.spec = Some(spec.to_string());
        self
    }

    /// UniMorph rows: `lemma<TAB>form<TAB>FEATURE;FEATURE`.
    pub fn unimorph(mut self, tsv: &str) -> Self {
        self.unimorph = Some(tsv.to_string());
        self
    }

    /// Words separated by whitespace.
    pub fn words(mut self, words: &str) -> Self {
        self.words = Some(words.to_string());
        self
    }

    /// Raw running text in the language.
    pub fn corpus(mut self, text: &str) -> Self {
        self.corpus = Some(text.to_string());
        self
    }

    /// Generate the lexer. It is an error to give nothing to build from, or
    /// both a spec and tables.
    pub fn build(self) -> Result<Lexer> {
        let base = match (self.spec, self.unimorph) {
            (Some(_), Some(_)) => return spec_error("give a spec or UniMorph tables, not both"),
            (Some(spec), None) => Some(spec),
            (None, Some(tsv)) => Some(crate::generate::from_unimorph(&tsv)?.spec),
            (None, None) => None,
        };
        let spec = match (base, &self.words) {
            (Some(spec), Some(words)) => extend(spec, words)?,
            (Some(spec), None) => spec,
            (None, Some(words)) => crate::discover::from_wordlist(words)?.spec,
            (None, None) => return spec_error("nothing to build from: give unimorph tables, a spec or words"),
        };
        match &self.corpus {
            Some(text) => Ok(Lexer::compile_with_corpus(&spec, text)?.0),
            None => Lexer::compile(&spec),
        }
    }
}

fn spec_error<T>(msg: &str) -> Result<T> {
    Err(Error::Spec { line: 0, msg: msg.into() })
}

/// Add paradigms discovered from the `words` the grammar in `spec` rejects.
fn extend(spec: String, words: &str) -> Result<String> {
    let first = Lexer::compile(&spec)?;
    let missed: Vec<String> =
        first.lex_text(words, Mode::Strict).into_iter().filter(|(_, out)| matches!(out, WordOutput::Fallback(_))).map(|(w, _)| w).collect();
    match crate::discover::discover_named(&missed.join("\n"), "D_") {
        Ok(found) => splice(&spec, &found.spec),
        Err(_) => Ok(spec),
    }
}

/// Put the discovered spec's root continuations into `base`'s `Root`
/// lexicon and append its other lexicons.
fn splice(base: &str, found: &str) -> Result<String> {
    const ROOT: &str = "LEXICON Root";
    let (Some(at), Some(root)) = (base.find(ROOT), found.find(ROOT)) else {
        return spec_error("the spec has no `LEXICON Root` to add discovered words to");
    };
    let (Some(eol), Some(from)) = (base.get(at..).and_then(|s| s.find('\n')), found.get(root..)) else {
        return spec_error("malformed `LEXICON Root`");
    };
    let Some((links, lexicons)) = from.split_once('\n').and_then(|(_, rest)| rest.split_once("\n\n")) else { return Ok(base.to_string()) };
    let cut = at + eol + 1;
    Ok(format!("{}{links}\n{}\n{lexicons}", base.get(..cut).unwrap_or(base), base.get(cut..).unwrap_or("")))
}
