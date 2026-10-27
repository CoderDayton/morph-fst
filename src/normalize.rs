//! Language-aware text normalization (applied by [`crate::Lexer::lex_text`] before
//! pre-tokenization; `Lexer::lex` expects already-normalized words).
//!
//! * Latin composition: base letter + combining mark → precomposed char
//!   (`e`+U+0301 → `é`), so decomposed input matches the spec.
//! * Case: `lower`, `turkish` (I→ı, İ→i, then lower) or `none`.
//! * Apostrophes: `’` → `'` when the language keeps apostrophes in words.
//! * Hangul: syllables U+AC00–D7A3 are decomposed arithmetically into
//!   conjoining jamo so a morpheme boundary may fall inside a syllable
//!   (가 + ᆻ = 갔). [`compose_hangul`] reverses this for display.

use crate::spec::CaseMode;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NormOptions {
    pub case: CaseMode,
    pub hangul_jamo: bool,
    pub apostrophe_in_words: bool,
    /// Drop tone marks (acute, grave, macron), keep dot below/above.
    pub strip_tones: bool,
}

const S_BASE: u32 = 0xAC00;
const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const L_COUNT: u32 = 19;
const V_COUNT: u32 = 21;
const T_COUNT: u32 = 28;
const S_COUNT: u32 = L_COUNT * V_COUNT * T_COUNT;

fn push_decomposed(out: &mut String, c: char) {
    let s = c as u32;
    if (S_BASE..S_BASE + S_COUNT).contains(&s) {
        let i = s - S_BASE;
        let (l, v, t) = (i / (V_COUNT * T_COUNT), (i % (V_COUNT * T_COUNT)) / T_COUNT, i % T_COUNT);
        // The jamo ranges are valid scalar values; keep `c` if that ever fails.
        let (Some(lc), Some(vc), Some(tc)) = (char::from_u32(L_BASE + l), char::from_u32(V_BASE + v), char::from_u32(T_BASE + t)) else {
            out.push(c);
            return;
        };
        out.push(lc);
        out.push(vc);
        if t > 0 {
            out.push(tc);
        }
    } else {
        out.push(c);
    }
}

pub fn decompose_hangul(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        push_decomposed(&mut out, c);
    }
    out
}

fn is_jamo(c: char) -> bool {
    ('\u{1100}'..='\u{11FF}').contains(&c)
}

/// Recompose conjoining jamo L V (T) into syllables; everything else as is.
pub fn compose_hangul(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.chars().any(is_jamo) {
        return std::borrow::Cow::Borrowed(s);
    }
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < cs.len() {
        let l = cs[i] as u32;
        if (L_BASE..L_BASE + L_COUNT).contains(&l) && i + 1 < cs.len() {
            let v = cs[i + 1] as u32;
            if (V_BASE..V_BASE + V_COUNT).contains(&v) {
                let start = i;
                let mut syl = S_BASE + ((l - L_BASE) * V_COUNT + (v - V_BASE)) * T_COUNT;
                i += 2;
                if let Some(&t) = cs.get(i) {
                    let t = t as u32;
                    if (T_BASE + 1..T_BASE + T_COUNT).contains(&t) {
                        syl += t - T_BASE;
                        i += 1;
                    }
                }
                // `syl` is inside U+AC00–D7A3; keep the jamo if that ever fails.
                match char::from_u32(syl) {
                    Some(s) => out.push(s),
                    None => out.extend(&cs[start..i]),
                }
                continue;
            }
        }
        out.push(cs[i]);
        i += 1;
    }
    std::borrow::Cow::Owned(out)
}

/// (composed, base, combining mark)
const LATIN: &[(char, char, char)] = &[
    ('à', 'a', '\u{300}'), ('á', 'a', '\u{301}'), ('â', 'a', '\u{302}'), ('ã', 'a', '\u{303}'), ('ä', 'a', '\u{308}'), ('å', 'a', '\u{30A}'),
    ('è', 'e', '\u{300}'), ('é', 'e', '\u{301}'), ('ê', 'e', '\u{302}'), ('ë', 'e', '\u{308}'),
    ('ì', 'i', '\u{300}'), ('í', 'i', '\u{301}'), ('î', 'i', '\u{302}'), ('ï', 'i', '\u{308}'),
    ('ò', 'o', '\u{300}'), ('ó', 'o', '\u{301}'), ('ô', 'o', '\u{302}'), ('õ', 'o', '\u{303}'), ('ö', 'o', '\u{308}'),
    ('ù', 'u', '\u{300}'), ('ú', 'u', '\u{301}'), ('û', 'u', '\u{302}'), ('ü', 'u', '\u{308}'),
    ('ý', 'y', '\u{301}'), ('ÿ', 'y', '\u{308}'), ('ñ', 'n', '\u{303}'), ('ç', 'c', '\u{327}'),
    ('ğ', 'g', '\u{306}'), ('ş', 's', '\u{327}'),
    ('À', 'A', '\u{300}'), ('Á', 'A', '\u{301}'), ('Â', 'A', '\u{302}'), ('Ã', 'A', '\u{303}'), ('Ä', 'A', '\u{308}'), ('Å', 'A', '\u{30A}'),
    ('È', 'E', '\u{300}'), ('É', 'E', '\u{301}'), ('Ê', 'E', '\u{302}'), ('Ë', 'E', '\u{308}'),
    ('Ì', 'I', '\u{300}'), ('Í', 'I', '\u{301}'), ('Î', 'I', '\u{302}'), ('Ï', 'I', '\u{308}'), ('İ', 'I', '\u{307}'),
    ('Ò', 'O', '\u{300}'), ('Ó', 'O', '\u{301}'), ('Ô', 'O', '\u{302}'), ('Õ', 'O', '\u{303}'), ('Ö', 'O', '\u{308}'),
    ('Ù', 'U', '\u{300}'), ('Ú', 'U', '\u{301}'), ('Û', 'U', '\u{302}'), ('Ü', 'U', '\u{308}'),
    ('Ý', 'Y', '\u{301}'), ('Ñ', 'N', '\u{303}'), ('Ç', 'C', '\u{327}'), ('Ğ', 'G', '\u{306}'), ('Ş', 'S', '\u{327}'),
];

/// The base letter of a precomposed Latin letter (`á` → `a`, `ị` → `i`);
/// other chars unchanged.
pub(crate) fn base_letter(c: char) -> char {
    LATIN.iter().chain(DOTTED).find(|&&(k, _, _)| k == c).map_or(c, |&(_, b, _)| b)
}

fn compose_pair(base: char, mark: char) -> Option<char> {
    LATIN.iter().find(|&&(_, b, m)| b == base && m == mark).map(|&(c, _, _)| c)
}

fn is_combining(c: char) -> bool {
    ('\u{300}'..='\u{36F}').contains(&c)
}

/// Tone marks removed by `strip_tones`.
fn is_tone(c: char) -> bool {
    matches!(c, '\u{300}' | '\u{301}' | '\u{304}')
}

/// Base letter + dot below / dot above → precomposed (Igbo, Yoruba ...).
const DOTTED: &[(char, char, char)] = &[
    ('ị', 'i', '\u{323}'), ('ụ', 'u', '\u{323}'), ('ọ', 'o', '\u{323}'), ('ẹ', 'e', '\u{323}'), ('ṣ', 's', '\u{323}'),
    ('Ị', 'I', '\u{323}'), ('Ụ', 'U', '\u{323}'), ('Ọ', 'O', '\u{323}'), ('Ẹ', 'E', '\u{323}'), ('Ṣ', 'S', '\u{323}'),
    ('ṅ', 'n', '\u{307}'), ('Ṅ', 'N', '\u{307}'),
];

/// Precomposed vowel + tone mark → base (only used with `strip_tones`).
fn untone(c: char) -> Option<char> {
    LATIN.iter().find(|&&(k, _, m)| k == c && is_tone(m)).map(|&(_, b, _)| b)
}

/// Strip tones and compose dot-below/above: decompose the word's marks,
/// drop tone marks, recompose (order-independent, so `ị́` = `i◌̣◌́` = `í◌̣`).
fn strip_tones(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut base: Option<char> = None;
    let mut marks: Vec<char> = Vec::new();
    let flush = |out: &mut String, base: &mut Option<char>, marks: &mut Vec<char>| {
        if let Some(mut b) = base.take() {
            for &m in marks.iter() {
                if let Some(&(k, _, _)) = DOTTED.iter().find(|&&(_, x, y)| x == b && y == m) {
                    b = k;
                } else if let Some(k) = compose_pair(b, m) {
                    b = k;
                } else {
                    out.push(b);
                    b = m;
                }
            }
            out.push(b);
        } else {
            out.extend(marks.iter());
        }
        marks.clear();
    };
    for c in text.chars() {
        if is_combining(c) {
            if !is_tone(c) {
                marks.push(c);
            }
            continue;
        }
        flush(&mut out, &mut base, &mut marks);
        base = Some(untone(c).unwrap_or(c));
    }
    flush(&mut out, &mut base, &mut marks);
    out
}

pub fn normalize(text: &str, o: &NormOptions) -> String {
    let stripped;
    let text = if o.strip_tones {
        stripped = strip_tones(text);
        stripped.as_str()
    } else {
        text
    };
    let mut out = String::with_capacity(text.len() + text.len() / 2);
    let mut prev: Option<char> = None;
    let flush = |out: &mut String, c: char| {
        let mut emit = |c: char| {
            if o.hangul_jamo {
                push_decomposed(out, c)
            } else {
                out.push(c)
            }
        };
        let c = if o.apostrophe_in_words && c == '\u{2019}' { '\'' } else { c };
        match o.case {
            CaseMode::None => emit(c),
            CaseMode::Turkish if c == 'I' => emit('ı'),
            CaseMode::Turkish if c == 'İ' => emit('i'),
            _ => {
                for l in c.to_lowercase() {
                    emit(l)
                }
            }
        }
    };
    for c in text.chars() {
        if is_combining(c) {
            if let Some(p) = prev {
                if let Some(k) = compose_pair(p, c) {
                    prev = Some(k);
                    continue;
                }
            }
        }
        if let Some(p) = prev.take() {
            flush(&mut out, p);
        }
        prev = Some(c);
    }
    if let Some(p) = prev {
        flush(&mut out, p);
    }
    out
}
