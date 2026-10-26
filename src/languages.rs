//! Bundled language specs (the `.morph` sources), ready to compile:
//!
//! ```
//! let lexer = morph_fst::Lexer::compile(morph_fst::languages::TURKISH)?;
//! # Ok::<(), morph_fst::Error>(())
//! ```
//!
//! They are compact demo lexicons (a few hundred morphemes each) showing the
//! spec language; extend them or write your own.

/// English: spelling rules (y→i, e-deletion, doubling, +es), derivation.
pub const ENGLISH: &str = include_str!("../data/english.morph");
/// Turkish: long-distance vowel harmony, consonant alternations.
pub const TURKISH: &str = include_str!("../data/turkish.morph");
/// Spanish: plural allomorphy, stem changes, orthographic alternations.
pub const SPANISH: &str = include_str!("../data/spanish.morph");
/// French: stem alternations, elision, feminine/plural morphology.
pub const FRENCH: &str = include_str!("../data/french.morph");
/// Korean (Hangul → jamo): particle allomorphy, 아/어 harmony, irregulars.
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

/// Look a bundled spec up by name (`"turkish"`, `"tr"`, ...).
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
