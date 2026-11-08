//! [`LexerBuilder`]: generate a language from whatever data exists for it.

use crate::error::{Error, Result};
use crate::fused::{Lexer, Mode};
use crate::output::WordOutput;
use crate::spec::Spec;

/// Describes a language to generate. Give it any of these, then
/// [`build`](Self::build):
///
/// * [`unimorph`](Self::unimorph): inflection tables with features and
///   lemmas (the richest source);
/// * [`hunspell`](Self::hunspell): a spell-checking dictionary, stems and
///   affix rules (about a hundred languages; affix classes stand in for
///   features). Next to tables, its rows are added to theirs;
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
    hunspell: Option<(String, String)>,
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

    /// A Hunspell dictionary: the `.aff` and `.dic` file contents, already
    /// decoded (see [`hunspell::decode`](crate::hunspell::decode)).
    pub fn hunspell(mut self, aff: &str, dic: &str) -> Self {
        self.hunspell = Some((aff.to_string(), dic.to_string()));
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
    /// both a spec and tables (or a dictionary).
    pub fn build(self) -> Result<Lexer> {
        let tables = match (self.unimorph, &self.hunspell) {
            (t, None) => t,
            (_, Some(_)) if self.spec.is_some() => return spec_error("give a spec or tables (UniMorph, Hunspell), not both"),
            (t, Some((aff, dic))) => {
                let mut t = t.unwrap_or_default();
                if !t.is_empty() && !t.ends_with('\n') {
                    t.push('\n');
                }
                t.push_str(&crate::hunspell::to_unimorph(aff, dic)?);
                Some(t)
            }
        };
        let base = match (self.spec, tables) {
            (Some(_), Some(_)) => return spec_error("give a spec or tables (UniMorph, Hunspell), not both"),
            (Some(spec), None) => Some(spec),
            (None, Some(tsv)) => Some(crate::generate::from_unimorph(&tsv)?.spec),
            (None, None) => None,
        };
        let spec = match (base, &self.words) {
            (Some(spec), Some(words)) => extend(spec, words)?,
            (Some(spec), None) => spec,
            (None, Some(words)) => crate::discover::from_wordlist(words)?.spec,
            (None, None) => return spec_error("nothing to build from: give unimorph tables, a hunspell dictionary, a spec or words"),
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
    let parsed = crate::spec::parse_spec(&spec)?;
    let first = Lexer::from_grammar(&crate::spec::compile_grammar(&parsed)?);
    let missed: Vec<String> =
        first.lex_text(words, Mode::Strict).into_iter().filter(|(_, out)| matches!(out, WordOutput::Fallback(_))).map(|(w, _)| w).collect();
    match crate::discover::discover_named(&missed.join("\n"), "D_", &first.mt.norm) {
        Ok((found, found_spec)) => splice(&spec, &parsed, &found.spec, &found_spec),
        Err(_) => Ok(spec),
    }
}

/// Put the discovered spec's root continuations into `base`'s `Root`
/// lexicon and append its other lexicons. Lexicons are found from the
/// parsed specs, so comments and spacing do not matter.
fn splice(base: &str, base_spec: &Spec, found: &str, found_spec: &Spec) -> Result<String> {
    // 0-based line of each lexicon header, in order, and which is `Root`.
    let heads = |s: &Spec| (s.lexicons.iter().map(|l| l.line - 1).collect::<Vec<_>>(), s.lexicons.iter().position(|l| l.name == "Root"));
    let ((base_heads, Some(b)), (found_heads, Some(f))) = (heads(base_spec), heads(found_spec)) else {
        return spec_error("the spec has no `LEXICON Root` to add discovered words to");
    };
    let found_lines: Vec<&str> = found.lines().collect();
    let root_end = found_heads.get(f + 1).copied().unwrap_or(found_lines.len());
    let (Some(links), Some(before), Some(after)) =
        (found_lines.get(found_heads[f] + 1..root_end), found_lines.get(found_heads[0]..found_heads[f]), found_lines.get(root_end..))
    else {
        return spec_error("malformed discovered spec");
    };
    let mut out: Vec<&str> = base.lines().collect();
    let tail = out.split_off((base_heads[b] + 1).min(out.len()));
    out.extend(links);
    out.extend(tail);
    out.push("");
    out.extend(before.iter().chain(after));
    Ok(out.join("\n") + "\n")
}
