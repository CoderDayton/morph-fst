//! Lexer output types (comparable with `==`).

use std::fmt;

use crate::normalize::compose_hangul as disp;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Morph {
    /// Byte span in the (normalized) word.
    pub start: usize,
    pub end: usize,
    pub surface: String,
    pub lemma: String,
    pub tag: String,
}

pub type Analysis = Vec<Morph>;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SubToken {
    /// A vocabulary piece; `cont` = word-internal (`##` piece).
    Piece { text: String, cont: bool },
    /// Byte fallback for a character outside the vocabulary alphabet.
    Byte(u8),
}

impl fmt::Display for SubToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubToken::Piece { text, cont: true } => write!(f, "##{}", disp(text)),
            SubToken::Piece { text, cont: false } => write!(f, "{}", disp(text)),
            SubToken::Byte(b) => write!(f, "<0x{b:02X}>"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    Morph(Morph),
    /// Hybrid mode: an unknown stem of an open class, split into pieces.
    Unknown { start: usize, end: usize, surface: String, guess: String, pieces: Vec<SubToken> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WordOutput {
    /// The morphological FST accepted the word.
    Analyzed { best: Analysis, all: Vec<Analysis> },
    /// FST rejected the word; deterministic subword split.
    Fallback(Vec<SubToken>),
    /// Hybrid mode: known affixes around one unknown stem.
    Hybrid { segments: Vec<Segment>, unknown_chars: u32 },
}

/// `lemma<TAG>+lemma<TAG>` (Hangul jamo recomposed for display).
pub fn analysis_string(a: &Analysis) -> String {
    a.iter().map(|m| format!("{}<{}>", disp(&m.lemma), m.tag)).collect::<Vec<_>>().join("+")
}

/// `surf|surf` (Hangul jamo recomposed per morpheme for display).
pub fn segmentation(a: &Analysis) -> String {
    a.iter().map(|m| disp(&m.surface).into_owned()).collect::<Vec<_>>().join("|")
}

pub fn pieces_string(p: &[SubToken]) -> String {
    p.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(" ")
}

/// Concatenate pieces back into bytes (lossless round trip).
pub fn reconstruct(p: &[SubToken]) -> Vec<u8> {
    let mut out = Vec::new();
    for t in p {
        match t {
            SubToken::Piece { text, .. } => out.extend_from_slice(text.as_bytes()),
            SubToken::Byte(b) => out.push(*b),
        }
    }
    out
}

impl WordOutput {
    pub fn source(&self) -> &'static str {
        match self {
            WordOutput::Analyzed { .. } => "FST",
            WordOutput::Fallback(_) => "SUB",
            WordOutput::Hybrid { .. } => "HYB",
        }
    }

    /// Flat token strings for downstream consumers.
    pub fn tokens(&self) -> Vec<String> {
        match self {
            WordOutput::Analyzed { best, .. } => best.iter().map(|m| format!("{}<{}>", disp(&m.lemma), m.tag)).collect(),
            WordOutput::Fallback(p) => p.iter().map(|t| t.to_string()).collect(),
            WordOutput::Hybrid { segments, .. } => segments
                .iter()
                .flat_map(|s| match s {
                    Segment::Morph(m) => vec![format!("{}<{}>", disp(&m.lemma), m.tag)],
                    Segment::Unknown { pieces, guess, .. } => {
                        let mut v: Vec<String> = pieces.iter().map(|t| t.to_string()).collect();
                        v.push(format!("<?{guess}>"));
                        v
                    }
                })
                .collect(),
        }
    }

    /// One-line human rendering.
    pub fn render(&self) -> String {
        match self {
            WordOutput::Analyzed { best, all } => {
                let mut s = format!("{}   [{}]", analysis_string(best), segmentation(best));
                if all.len() > 1 {
                    s.push_str(&format!("   (+{} more)", all.len() - 1));
                }
                s
            }
            WordOutput::Fallback(p) => pieces_string(p),
            WordOutput::Hybrid { segments, .. } => segments
                .iter()
                .map(|s| match s {
                    Segment::Morph(m) => format!("{}<{}>", disp(&m.lemma), m.tag),
                    Segment::Unknown { surface, guess, pieces, .. } => {
                        format!("⟨{}⟩<?{guess}>{{{}}}", disp(surface), pieces_string(pieces))
                    }
                })
                .collect::<Vec<_>>()
                .join(" + "),
        }
    }
}
