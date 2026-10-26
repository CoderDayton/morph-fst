//! Pre-tokenization: split (normalized) running text into words.
//!
//! Whitespace separates; each punctuation character is its own token.
//! Everything else (letters, digits, combining marks, emoji, CJK) stays in
//! the word — the lexer handles it, with byte fallback as the last resort.
//! `std` has no Unicode general-category tables, so punctuation is ASCII
//! punctuation plus the main Unicode punctuation blocks.

pub fn is_punct(c: char) -> bool {
    c.is_ascii_punctuation()
        || matches!(c,
            '\u{00A1}' | '\u{00A7}' | '\u{00AB}' | '\u{00B6}' | '\u{00B7}' | '\u{00BB}' | '\u{00BF}'
            | '\u{2010}'..='\u{2027}' | '\u{2030}'..='\u{205E}'
            | '\u{3001}'..='\u{3003}' | '\u{3008}'..='\u{3011}' | '\u{3014}'..='\u{301F}'
            | '\u{FF01}'..='\u{FF0F}' | '\u{FF1A}'..='\u{FF20}' | '\u{FF3B}'..='\u{FF40}' | '\u{FF5B}'..='\u{FF65}')
}

/// Byte spans of words/punctuation in `text`.
/// With `apostrophe_in_words`, `'` is a word character (`l'homme`).
pub fn split(text: &str, apostrophe_in_words: bool) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() || (is_punct(c) && !(apostrophe_in_words && c == '\'')) {
            if let Some(s) = start.take() {
                out.push((s, i));
            }
            if !c.is_whitespace() {
                out.push((i, i + c.len_utf8()));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push((s, text.len()));
    }
    out
}
