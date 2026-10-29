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
    detail: WordOutput,
    span: Range<usize>,
}

impl Word {
    pub(crate) fn new(text: String, span: Range<usize>, detail: WordOutput) -> Self {
        let (kind, mut readings) = match &detail {
            WordOutput::Analyzed { all, .. } => (Kind::Known, all.iter().map(|a| a.iter().map(known).collect()).collect::<Vec<Vec<Part>>>()),
            WordOutput::Hybrid { all, .. } => (Kind::Guessed, all.iter().map(|r| r.iter().map(segment).collect()).collect()),
            WordOutput::Fallback(pieces) => (Kind::Unsplit, vec![pieces.iter().map(piece).collect()]),
        };
        let parts = if readings.is_empty() { Vec::new() } else { readings.remove(0) };
        Word { text, kind, parts, others: readings, detail, span }
    }

    pub(crate) fn literal(text: &str, class: crate::Class, span: Range<usize>) -> Self {
        let detail = WordOutput::Fallback(vec![SubToken::Piece { text: text.to_string(), cont: false }]);
        let part = Part { text: text.to_string(), lemma: text.to_string(), tag: class.tag().to_string(), known: false };
        Word { text: text.to_string(), kind: Kind::Literal, parts: vec![part], others: Vec::new(), detail, span }
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
