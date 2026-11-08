//! Abstract syntax of the lexc-like `.morph` language.
//!
//! Beyond plain lexicons, a spec may declare
//! * `CLASS` sets of symbols (chars and archiphonemes),
//! * `FEATURE` blocks — a deterministic automaton over surface characters
//!   (the *context register*, e.g. "last vowel is back/front"),
//! * `ARCHI` archiphonemes (`{A}`, `{I}`, ...) realised from the register,
//! * `RULES` with non-final targets, class contexts, optional rules.

/// A symbol of a lexical template: a literal char or an archiphoneme.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Sym {
    Ch(char),
    Archi(String),
}

/// Parse `l{A}r` into `[Ch(l), Archi(A), Ch(r)]`.
pub fn parse_template(s: &str) -> Result<Vec<Sym>, String> {
    let mut out = Vec::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        match c {
            '{' => {
                let mut name = String::new();
                loop {
                    match it.next() {
                        Some('}') => break,
                        Some('{') | None => return Err(format!("unterminated archiphoneme in `{s}`")),
                        Some(x) => name.push(x),
                    }
                }
                if name.is_empty() {
                    return Err(format!("empty archiphoneme `{{}}` in `{s}`"));
                }
                out.push(Sym::Archi(name));
            }
            '}' => return Err(format!("stray `}}` in `{s}`")),
            _ => out.push(Sym::Ch(c)),
        }
    }
    Ok(out)
}

/// Template text with braces removed (`l{A}r` → `lAr`): default lemma.
pub fn strip_braces(s: &str) -> String {
    s.chars().filter(|&c| c != '{' && c != '}').collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpecEntry {
    /// Lexical template; empty string = epsilon (written `0`).
    pub surface: String,
    pub lemma: String,
    /// Empty for a plain epsilon; set for a zero morpheme (`0 TAG Cont ;`).
    pub tag: String,
    /// Continuation lexicon name, or `#` for end-of-word.
    pub cont: String,
    pub flags: Vec<String>,
    /// Tags of the zero morphemes that follow this one (set when zeros are
    /// folded into the morpheme before them; see [`crate::spec::zeros`]).
    pub zeros: Vec<String>,
    pub line: usize,
}

impl SpecEntry {
    /// Empty surface: a plain epsilon or a zero morpheme.
    pub fn is_epsilon(&self) -> bool {
        self.surface.is_empty()
    }
    /// A zero morpheme: empty surface with a tag.
    pub fn is_zero(&self) -> bool {
        self.surface.is_empty() && !self.tag.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpecLexicon {
    pub name: String,
    /// `@open=TAG`: hybrid mode may posit an unknown stem of this class.
    pub open_tag: Option<String>,
    pub entries: Vec<SpecEntry>,
    pub line: usize,
}

/// One element of a character pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Atom {
    Ch(char),
    /// `[NAME]`
    Class(String),
    /// `[*]`
    Any,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quant {
    One,
    /// `+`
    Plus,
    /// `*`
    Star,
    /// `?`
    Opt,
}

/// Conjunction of `feature=v1|v2` tests on the context register.
pub type Cond = Vec<(String, Vec<String>)>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassDef {
    pub name: String,
    pub members: Vec<Sym>,
    pub line: usize,
}

/// `[<cond>] atoms -> value`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeatLine {
    pub cond: Cond,
    pub atoms: Vec<Atom>,
    pub value: String,
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeatureDef {
    pub name: String,
    pub lines: Vec<FeatLine>,
    /// `RFEATURE`: computed right-to-left, i.e. its value at a position is a
    /// function of the text *after* it (regressive harmony).
    pub right: bool,
    pub line: usize,
}

/// `cond -> output` (`*` = unconditional). Output `0` = empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiLine {
    pub cond: Option<Cond>,
    pub output: String,
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiDef {
    pub name: String,
    pub lines: Vec<ArchiLine>,
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeftCtx {
    Any,
    /// Built-in `C` (or a spec-defined CLASS C).
    Consonant,
    /// Built-in `V` (or a spec-defined CLASS V).
    Vowel,
    /// Target starts the entry.
    Boundary,
    Class(String),
    /// The text before the target ends with this string.
    Literal(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// `.` — any single char (bound to `$`).
    AnyChar,
    /// `[NAME]` — any single char of a class (bound to `$`).
    Class(String),
    Literal(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RightCtx {
    /// `+{s1,s2}`: the next morpheme's lexical template is one of these.
    Lex(Vec<String>),
    /// `+[NAME]`: the next morpheme's first lexical symbol is in the class.
    Class(String),
}

/// `name[?]: TARGET -> REPL / [LEFT] _ [TAIL...] +RIGHT [@flag] ;`
///
/// The rightmost occurrence of TARGET whose following text in the entry
/// matches TAIL exactly (anchored at the entry's end) is rewritten to REPL,
/// whenever the next morpheme matches RIGHT. Rules apply sequentially in
/// spec order to the current string; `name?:` marks an optional rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpecRule {
    pub name: String,
    pub optional: bool,
    pub target: Target,
    /// Replacement; `$` expands to the matched target. Empty = deletion.
    pub replacement: String,
    pub left: LeftCtx,
    pub tail: Vec<(Atom, Quant)>,
    pub right: RightCtx,
    pub require_flag: Option<String>,
    pub line: usize,
}

/// English-style built-in vowel test (used when no CLASS V is declared).
pub fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'i' | 'o' | 'u')
}

/// English-style built-in consonant test (no CLASS C declared).
pub fn is_consonant(c: char) -> bool {
    c.is_alphabetic() && !is_vowel(c)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CaseMode {
    #[default]
    Lower,
    /// Turkish/Azeri: I→ı, İ→i, then lowercase.
    Turkish,
    None,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// Words longer than this (in chars, after normalization) skip
    /// morphological analysis and go straight to the subword fallback.
    pub max_morph_chars: usize,
    /// Also make suffix morphemes word-initial pieces in the derived vocab.
    pub suffix_initial_pieces: bool,
    pub case: CaseMode,
    /// Decompose Hangul syllables into conjoining jamo (spec and input).
    pub hangul_jamo: bool,
    /// Keep `'` (and `’`, mapped to `'`) inside words, e.g. French `l'homme`.
    pub apostrophe_in_words: bool,
    /// Remove tone marks (acute, grave, macron) but keep dot-below/above
    /// (Igbo `ị́` → `ị`).
    pub strip_tones: bool,
    /// Remove Arabic and Hebrew vowel marks (spec and input).
    pub strip_vowel_marks: bool,
    /// `surface` (default): registers read the surface text. `underlying`:
    /// registers read each morpheme's underlying form (its realisation
    /// before rules, or an explicit `@tier=` form) — opaque harmony.
    pub underlying: bool,
    /// Minimum corpus evidence (token count) for an induced exception.
    pub induce_min_count: usize,
    /// Tags of derivational morphemes (`OPTION derivation PFX,NMLZ`). A tag
    /// matches itself and its `TAG:sub` forms. Everything else is inflection:
    /// a word's lemma keeps derivation and drops inflection.
    pub derivation: Vec<String>,
    /// `PREFER` lines: which reading a word takes next to another word,
    /// in order (the first that applies decides).
    pub context: Vec<ContextRule>,
}

/// What a context rule looks for in a word's best reading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sel {
    /// The reading's class: the tag of its last derivation, or of its stem.
    /// A tag also matches its `TAG:sub` forms.
    Tag(String),
    /// The word's lemma (`"to"`, `"be"`).
    Lemma(String),
}

/// `PREFER prefer [OVER over] AFTER|BEFORE near`: when the word before (or
/// after) matches `near`, a word whose best reading matches `over` (any, if
/// empty) but not `prefer` takes its first reading that matches `prefer`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextRule {
    pub prefer: Vec<Sel>,
    pub over: Vec<Sel>,
    pub before: bool,
    pub near: Vec<Sel>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            max_morph_chars: 64,
            suffix_initial_pieces: false,
            case: CaseMode::Lower,
            hangul_jamo: false,
            apostrophe_in_words: false,
            strip_tones: false,
            strip_vowel_marks: false,
            underlying: false,
            induce_min_count: 2,
            derivation: Vec::new(),
            context: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Spec {
    pub lexicons: Vec<SpecLexicon>,
    pub rules: Vec<SpecRule>,
    pub classes: Vec<ClassDef>,
    pub features: Vec<FeatureDef>,
    pub archis: Vec<ArchiDef>,
    /// Raw extra pieces; a leading `##` marks a word-internal piece.
    pub pieces: Vec<(String, usize)>,
    pub options: Options,
    /// `INDUCE f1 f2 IN Lex1 Lex2`: roots in these lexicons may carry a
    /// lexical (exceptional) value of these features. Values are learned
    /// from a corpus at build time and guessed for unknown stems in hybrid
    /// mode — no hand marking.
    pub induce: Vec<InduceDef>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InduceDef {
    pub features: Vec<String>,
    pub lexicons: Vec<String>,
    pub line: usize,
}
