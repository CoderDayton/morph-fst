//! Bundled language specs (the `.morph` sources), packed into the library
//! and ready to compile:
//!
//! ```
//! let lexer = morph_fst::Lexer::compile(morph_fst::languages::ENGLISH)?;
//! # Ok::<(), morph_fst::Error>(())
//! ```
//!
//! English, Turkish, Spanish, French and Korean carry full lexicons; Igbo
//! is a compact demo lexicon showing the spec language.

/// English: spelling rules (y→i, e-deletion, doubling, +es), derivation.
pub const ENGLISH: &str = include_str!("../data/english.morph");
/// Turkish: long-distance vowel harmony, consonant alternations; lexicon
/// from Zemberek-NLP (Apache-2.0).
pub const TURKISH: &str = include_str!("../data/turkish.morph");
/// Spanish: plural allomorphy, stem changes, orthographic alternations;
/// lexicon from RLA-ES (MPL-1.1).
pub const SPANISH: &str = include_str!("../data/spanish.morph");
/// French: stem alternations, elision, feminine/plural morphology;
/// lexicon from Grammalecte (MPL-2.0).
pub const FRENCH: &str = include_str!("../data/french.morph");
/// Korean (Hangul → jamo): particle allomorphy, 아/어 harmony, irregulars;
/// lexicon from mecab-ko-dic (Apache-2.0).
pub const KOREAN: &str = include_str!("../data/korean.morph");
/// Igbo (simplified): ATR harmony in both directions.
pub const IGBO: &str = include_str!("../data/igbo.morph");

/// All bundled specs as (name, source).
pub const ALL: &[(&str, &str)] = &[
    ("english", ENGLISH),
    ("turkish", TURKISH),
    ("spanish", SPANISH),
    ("french", FRENCH),
    ("korean", KOREAN),
    ("igbo", IGBO),
];

/// The names in [`ALL`], without pulling the specs into a binary that only
/// needs the names (error messages).
pub(crate) const NAMES: &[&str] = &["english", "turkish", "spanish", "french", "korean", "igbo"];

/// Look a bundled spec up by name (`"english"`, `"tr"`, ...).
pub fn by_name(name: &str) -> Option<&'static str> {
    let n = name.to_ascii_lowercase();
    let key = match n.as_str() {
        "en" | "eng" => "english",
        "tr" | "tur" => "turkish",
        "es" | "spa" => "spanish",
        "fr" | "fra" => "french",
        "ko" | "kor" => "korean",
        "ig" | "ibo" => "igbo",
        other => other,
    };
    ALL.iter().find(|(k, _)| *k == key).map(|(_, s)| *s)
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_match_all() {
        let all: Vec<&str> = super::ALL.iter().map(|(k, _)| *k).collect();
        assert_eq!(all, super::NAMES);
    }
}
