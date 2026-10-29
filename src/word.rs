//! The everyday view of a lexed word: [`Word`] and its [`Part`]s.

use std::fmt;
use std::ops::Range;

use crate::output::{Segment, SubToken, WordOutput};

/// How well the grammar understood a word.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Kind {
    /// Every part is in the grammar.
    Known,
    /// Known endings around a stem the grammar has not seen; the stem's
    /// lemma and class are inferred from similar known stems.
    Guessed,
    /// The grammar had no reading, so the word was cut into subword pieces.
    Unsplit,
    /// A capitalized word the grammar does not know, in mid-sentence: a
    /// name. Kept whole (one part, tag `NAME`), never cut at an ending, so
    /// `Texas` is not `Texa` + `s`.
    Name,
    /// Not a word: a number, URL, email, abbreviation, clitic, punctuation
    /// or symbol. Kept whole and not looked up; the part's tag says which.
    Literal,
}

/// One piece of a word: a stem, an ending, or (for [`Kind::Unsplit`]) a subword piece.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    text: String,
    lemma: String,
    tag: String,
    known: bool,
}

impl Part {
    /// The letters as they appear in the word. Empty for a zero morpheme
    /// (a feature with no ending, like singular).
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The dictionary form of this part (`happy` for `happi` in `unhappiest`).
    pub fn lemma(&self) -> &str {
        &self.lemma
    }

    /// What the part is: `PL`, `PST`, `N`... For a guessed stem, the class it was guessed to be.
    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// Whether the grammar knows this part (false for a guessed stem or a subword piece).
    pub fn is_known(&self) -> bool {
        self.known
    }
}

/// A lexed word: its best reading, the other readings, and how sure the grammar was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Word {
    text: String,
    kind: Kind,
    parts: Vec<Part>,
    others: Vec<Vec<Part>>,
    lemma: Option<String>,
    detail: WordOutput,
    span: Range<usize>,
}

impl Word {
    pub(crate) fn new(text: String, span: Range<usize>, detail: WordOutput, policy: &Policy) -> Self {
        let (kind, readings) = match &detail {
            WordOutput::Analyzed { all, .. } => (Kind::Known, all.iter().map(|a| a.iter().map(known).collect()).collect::<Vec<Vec<Part>>>()),
            WordOutput::Hybrid { all, .. } => (Kind::Guessed, all.iter().map(|r| r.iter().map(segment).collect()).collect()),
            WordOutput::Fallback(pieces) => (Kind::Unsplit, vec![pieces.iter().map(piece).collect()]),
        };
        let mut distinct: Vec<Vec<Part>> = Vec::new();
        for r in readings {
            if !distinct.contains(&r) {
                distinct.push(r);
            }
        }
        policy.rank(&mut distinct);
        let parts = if distinct.is_empty() { Vec::new() } else { distinct.remove(0) };
        let lemma = (kind != Kind::Unsplit && !parts.is_empty()).then(|| whole_lemma(&parts, &policy.derivation));
        Word { text, kind, parts, others: distinct, lemma, detail, span }
    }

    /// A guessed word that turned out to be a name: whole, one part.
    pub(crate) fn into_name(self) -> Self {
        let part = Part { text: self.text.clone(), lemma: self.text.clone(), tag: "NAME".to_string(), known: false };
        Word { kind: Kind::Name, parts: vec![part], others: Vec::new(), lemma: None, ..self }
    }

    pub(crate) fn literal(text: &str, class: crate::Class, span: Range<usize>) -> Self {
        let detail = WordOutput::Fallback(vec![SubToken::Piece { text: text.to_string(), cont: false }]);
        let part = Part { text: text.to_string(), lemma: text.to_string(), tag: class.tag().to_string(), known: false };
        Word { text: text.to_string(), kind: Kind::Literal, parts: vec![part], others: Vec::new(), lemma: None, detail, span }
    }

    /// The word, normalized for the language (lowercased, composed, ...).
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// Where the word sits in the text it came from, in bytes. For
    /// [`Lexer::analyze`](crate::Lexer::analyze) it is `0..word.len()`.
    pub fn span(&self) -> Range<usize> {
        self.span.clone()
    }

    /// The dictionary form of the whole word, from its best reading:
    /// inflection is dropped (`runners` → `runner`), derivation is kept
    /// (`really`, `helpful`, `unhappy`). Which endings are derivation is the
    /// grammar's `OPTION derivation`; without it every ending is inflection.
    /// A derived lemma is spelled as the word spells it, so it is only as
    /// clean as the grammar's spelling rules. For [`Kind::Unsplit`] and
    /// [`Kind::Literal`] it is the text itself.
    pub fn lemma(&self) -> &str {
        self.lemma.as_deref().unwrap_or(&self.text)
    }

    /// The best reading, left to right.
    pub fn parts(&self) -> &[Part] {
        &self.parts
    }

    /// Other readings, best first. Empty when the word is unambiguous.
    pub fn alternatives(&self) -> &[Vec<Part>] {
        &self.others
    }

    /// Flat tokens for a downstream model: `["ev<N>", "lAr<PL>", "DA<LOC>"]`.
    pub fn tokens(&self) -> Vec<String> {
        self.detail.tokens()
    }

    /// The full result, with byte spans, subword pieces and every reading.
    pub fn detail(&self) -> &WordOutput {
        &self.detail
    }
}

impl fmt::Display for Word {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.detail.render())
    }
}

/// What a grammar says about its own tags: which are derivation, and which
/// categories are open (new words keep arriving, so a listed entry that
/// merely looks inflected is a lexicalization).
pub(crate) struct Policy {
    derivation: Vec<String>,
    open: Vec<String>,
}

impl Policy {
    pub(crate) fn of(mt: &crate::morphotactics::Morphotactics) -> Self {
        let mut open: Vec<String> = mt.classes.iter().filter_map(|c| c.open_tag.clone()).collect();
        open.sort();
        open.dedup();
        Policy { derivation: mt.derivation.clone(), open }
    }

    /// Among equally good readings, a productive stem + inflection (`go` +
    /// `ing`) outranks an open-class entry that is itself the inflected
    /// spelling (`going<N>`). Order is otherwise kept.
    fn rank(&self, readings: &mut [Vec<Part>]) {
        let inflected = |r: &[Part]| r.len() >= 2 && r.last().is_some_and(|p| p.known && !derivational(&p.tag, &self.derivation));
        let lexicalized = |r: &[Part]| matches!(r, [p] if p.known && self.open.contains(&p.tag));
        if readings.iter().any(|r| inflected(r)) {
            readings.sort_by_key(|r| lexicalized(r));
        }
    }
}

/// Derivation changes the word (prefixes, nominalizers); plural, tense and
/// comparison do not. The grammar lists its derivational tags; a listed tag
/// also covers its `TAG:sub` forms.
fn derivational(tag: &str, listed: &[String]) -> bool {
    listed.iter().any(|d| tag == d || tag.strip_prefix(d.as_str()).is_some_and(|rest| rest.starts_with(':')))
}

fn whole_lemma(parts: &[Part], listed: &[String]) -> String {
    let stem = parts.iter().position(|p| !derivational(&p.tag, listed)).unwrap_or(0);
    let last_derived = parts.iter().enumerate().skip(stem + 1).filter(|(_, p)| derivational(&p.tag, listed)).map(|(i, _)| i).next_back();
    match last_derived {
        Some(end) => parts.iter().take(end + 1).map(|p| p.text.as_str()).collect(),
        None => parts.iter().take(stem).map(|p| p.text.as_str()).chain(parts.get(stem).map(|p| p.lemma.as_str())).collect(),
    }
}

fn known(m: &crate::output::Morph) -> Part {
    Part { text: m.surface.clone(), lemma: m.lemma.clone(), tag: m.tag.clone(), known: true }
}

fn segment(s: &Segment) -> Part {
    match s {
        Segment::Morph(m) => known(m),
        Segment::Unknown { surface, lemma, guess, .. } => Part { text: surface.clone(), lemma: lemma.clone(), tag: guess.clone(), known: false },
    }
}

fn piece(t: &SubToken) -> Part {
    let text = match t {
        SubToken::Piece { text, .. } => text.clone(),
        SubToken::Byte(b) => format!("<0x{b:02X}>"),
    };
    Part { lemma: text.clone(), text, tag: String::new(), known: false }
}
