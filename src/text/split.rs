//! [`Split`]: the ways [`Lexer::split`](crate::Lexer::split) cuts text into
//! tokens.

use crate::normalize::compose_hangul;
use crate::output::{Segment, SubToken, WordOutput};
use crate::{Kind, Lexer, Word};

/// How [`Lexer::split`] cuts text.
///
/// ```
/// use morph_fst::{Lexer, Split};
///
/// let lx = Lexer::bundled("english")?;
/// let text = "The unhappiest cats walked.";
/// assert_eq!(lx.split(text, Split::Words), ["The", "unhappiest", "cats", "walked", "."]);
/// assert_eq!(lx.split(text, Split::Morphemes), ["the", "un", "happi", "est", "cat", "s", "walk", "ed", "."]);
/// assert_eq!(lx.split(text, Split::Lemmas), ["the", "unhappy", "cat", "walk"]);
/// assert_eq!(lx.split(text, Split::Pieces), ["the", "un", "##happi", "##est", "cat", "##s", "walk", "##ed", "."]);
/// # Ok::<(), morph_fst::Error>(())
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Split {
    /// Words, numbers, URLs and punctuation, as written.
    #[default]
    Words,
    /// Morphemes, normalized (`un happi est`); literals stay whole.
    Morphemes,
    /// One lemma per word, as [`Lexer::terms`]; punctuation is dropped.
    Lemmas,
    /// WordPiece-style pieces (`un ##happi ##est`): known morphemes whole,
    /// else the grammar's pieces, else bytes (`<0xNN>`).
    Pieces,
}

impl Lexer {
    /// Cut `text` into tokens of one kind: words, morphemes, lemmas or
    /// subword pieces (see [`Split`]). Built on
    /// [`analyze_text`](Self::analyze_text), so the language's
    /// normalization, context rules and guessing all apply.
    pub fn split(&self, text: &str, how: Split) -> Vec<String> {
        let words = self.analyze_text(text);
        match how {
            Split::Words => words.iter().filter_map(|w| text.get(w.span())).map(str::to_string).collect(),
            Split::Morphemes => words.iter().flat_map(morphemes).collect(),
            Split::Lemmas => words.iter().filter_map(|w| self.lemma_key(w)).collect(),
            Split::Pieces => words.iter().flat_map(pieces).collect(),
        }
    }

    /// The search key of a word, or `None` for punctuation, symbols and
    /// clitics.
    pub(crate) fn lemma_key(&self, w: &Word) -> Option<String> {
        const KEPT: [&str; 4] = ["NUM", "URL", "EMAIL", "ABBR"];
        let keep = w.kind() != Kind::Literal || w.parts().first().is_some_and(|p| KEPT.contains(&p.tag()));
        let key = crate::normalize::normalize(w.lemma(), &self.mt.norm);
        (keep && !key.is_empty()).then(|| compose_hangul(&key).into_owned())
    }
}

fn morphemes(w: &Word) -> Vec<String> {
    let parts: Vec<String> = w.parts().iter().filter(|p| !p.text().is_empty()).map(|p| compose_hangul(p.text()).into_owned()).collect();
    if parts.is_empty() {
        vec![w.text().to_string()]
    } else {
        parts
    }
}

/// The word's pieces, from the reading `parts()` shows (`detail` is kept in
/// step with it).
fn pieces(w: &Word) -> Vec<String> {
    let piece = |text: &str| SubToken::Piece { text: text.to_string(), cont: false };
    let raw: Vec<SubToken> = match w.detail() {
        WordOutput::Analyzed { best, .. } => best.iter().filter(|m| !m.surface.is_empty()).map(|m| piece(&m.surface)).collect(),
        WordOutput::Hybrid { segments, .. } => segments
            .iter()
            .flat_map(|s| match s {
                Segment::Morph(m) if m.surface.is_empty() => Vec::new(),
                Segment::Morph(m) => vec![piece(&m.surface)],
                Segment::Unknown { pieces, .. } => pieces.clone(),
            })
            .collect(),
        WordOutput::Fallback(p) => p.clone(),
    };
    raw.iter()
        .enumerate()
        .map(|(i, t)| match t {
            SubToken::Piece { text, .. } if i == 0 => compose_hangul(text).into_owned(),
            SubToken::Piece { text, .. } => format!("##{}", compose_hangul(text)),
            SubToken::Byte(b) => format!("<0x{b:02X}>"),
        })
        .collect()
}
