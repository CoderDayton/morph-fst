//! Hunspell dictionaries (`.aff` affix rules + `.dic` stems) as UniMorph
//! rows, so the generator builds a grammar from them.
//!
//! Every stem is expanded with the affix classes its flags name, as
//! `unmunch` does: strip, add, and the rule's condition on the stem's end
//! (suffixes) or start (prefixes); a suffix's own continuation flags apply
//! once more, and prefixes combine with suffixes when both allow it. Each
//! form becomes `lemma<TAB>form<TAB>features`, the features being the part
//! of speech from the stem's `po:` field (`STEM`, an open class, without
//! one) and one
//! `SFX_<flag>` or `PFX_<flag>` per affix. The generator then finds the
//! stems and paradigm classes, and hybrid mode reads unseen stems from
//! them. Flags may be single chars (default), `FLAG long`, `FLAG num` or
//! `FLAG UTF-8`, and may be aliased with `AF`. `FORBIDDENWORD` stems are
//! skipped and `NEEDAFFIX` stems get no bare form. Compounding,
//! `ICONV`/`OCONV`, `IGNORE` and morphological aliases (`AM`) are not read.

use std::collections::{BTreeSet, HashMap};

use crate::error::{Error, Result};

#[derive(Clone, Copy)]
enum FlagMode {
    Char,
    Long,
    Num,
}

enum CharSet {
    Any,
    In(Vec<char>),
    NotIn(Vec<char>),
}

impl CharSet {
    fn has(&self, c: char) -> bool {
        match self {
            CharSet::Any => true,
            CharSet::In(v) => v.contains(&c),
            CharSet::NotIn(v) => !v.contains(&c),
        }
    }
}

struct Rule {
    strip: String,
    add: String,
    cond: Vec<CharSet>,
    cont: Vec<String>,
}

struct Class {
    prefix: bool,
    cross: bool,
    rules: Vec<Rule>,
}

struct Aff {
    mode: FlagMode,
    aliases: Vec<Vec<String>>,
    classes: HashMap<String, Class>,
    forbidden: Option<String>,
    need_affix: Option<String>,
}

fn err<T>(line: usize, msg: &str) -> Result<T> {
    Err(Error::Spec { line, msg: format!("hunspell .aff: {msg}") })
}

/// Text of a Hunspell file in the encoding its `.aff` names with `SET`:
/// UTF-8, ISO8859-1 or ISO8859-15. Convert others with `iconv` first.
pub fn decode(bytes: &[u8], aff: &[u8]) -> Result<String> {
    let (bytes, aff) = (no_bom(bytes), no_bom(aff));
    let set = aff
        .split(|&b| b == b'\n')
        .find_map(|l| l.strip_prefix(b"SET").map(|v| String::from_utf8_lossy(v).trim().to_ascii_uppercase()))
        .unwrap_or_else(|| "ISO8859-1".to_string());
    match set.as_str() {
        "UTF-8" | "UTF8" => String::from_utf8(bytes.to_vec()).map_err(|_| Error::Io("hunspell file is not valid UTF-8".into())),
        "ISO8859-1" | "ISO-8859-1" => Ok(bytes.iter().map(|&b| b as char).collect()),
        "ISO8859-15" | "ISO-8859-15" => Ok(bytes.iter().map(|&b| latin9(b)).collect()),
        other => Err(Error::Io(format!("hunspell encoding {other} is not read; convert the files to UTF-8 with iconv"))),
    }
}

fn no_bom(b: &[u8]) -> &[u8] {
    b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(b)
}

fn latin9(b: u8) -> char {
    match b {
        0xA4 => '€',
        0xA6 => 'Š',
        0xA8 => 'š',
        0xB4 => 'Ž',
        0xB8 => 'ž',
        0xBC => 'Œ',
        0xBD => 'œ',
        0xBE => 'Ÿ',
        _ => b as char,
    }
}

fn cond(s: &str) -> Option<Vec<CharSet>> {
    let mut out = Vec::new();
    let mut cs = s.chars();
    while let Some(c) = cs.next() {
        out.push(match c {
            '.' => CharSet::Any,
            '[' => {
                let mut set: Vec<char> = Vec::new();
                let mut neg = false;
                loop {
                    match cs.next()? {
                        ']' => break,
                        '^' if set.is_empty() && !neg => neg = true,
                        x => set.push(x),
                    }
                }
                if neg {
                    CharSet::NotIn(set)
                } else {
                    CharSet::In(set)
                }
            }
            x => CharSet::In(vec![x]),
        });
    }
    Some(out)
}

impl Aff {
    fn flags(&self, s: &str) -> Vec<String> {
        resolve(&self.aliases, self.mode, s)
    }

    fn parse(text: &str) -> Result<Aff> {
        let mut aff = Aff { mode: FlagMode::Char, aliases: Vec::new(), classes: HashMap::new(), forbidden: None, need_affix: None };
        let mut af_count = false;
        for (i, raw) in text.lines().enumerate() {
            let line = i + 1;
            let toks: Vec<&str> = raw.split_whitespace().collect();
            match toks.as_slice() {
                ["FLAG", m, ..] => {
                    aff.mode = match *m {
                        "long" => FlagMode::Long,
                        "num" => FlagMode::Num,
                        _ => FlagMode::Char,
                    }
                }
                ["AF", f, ..] => {
                    if af_count {
                        aff.aliases.push(split_flags(aff.mode, f));
                    } else {
                        af_count = true;
                    }
                }
                ["FORBIDDENWORD", f, ..] => aff.forbidden = Some((*f).to_string()),
                ["NEEDAFFIX", f, ..] => aff.need_affix = Some((*f).to_string()),
                [kind @ ("PFX" | "SFX"), flag, rest @ ..] => {
                    let prefix = *kind == "PFX";
                    match aff.classes.get_mut(*flag) {
                        None => {
                            let cross = rest.first() == Some(&"Y");
                            aff.classes.insert((*flag).to_string(), Class { prefix, cross, rules: Vec::new() });
                        }
                        Some(class) => {
                            let [strip, add, more @ ..] = rest else { return err(line, "affix rule needs strip and add") };
                            let (add, cont) = match add.split_once('/') {
                                Some((a, f)) => (a, f),
                                None => (*add, ""),
                            };
                            let zero = |s: &str| if s == "0" { String::new() } else { s.to_string() };
                            let Some(cond) = cond(more.first().copied().unwrap_or(".")) else { return err(line, "bad condition") };
                            let cont = if cont.is_empty() { Vec::new() } else { resolve(&aff.aliases, aff.mode, cont) };
                            class.rules.push(Rule { strip: zero(strip), add: zero(add), cond, cont });
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(aff)
    }

    /// `word` with suffix `r` applied, if its condition holds.
    fn suffix(word: &str, r: &Rule) -> Option<String> {
        let mut cs = word.chars().rev();
        if !r.cond.iter().rev().all(|set| cs.next().is_some_and(|c| set.has(c))) {
            return None;
        }
        // Hunspell keeps at least one stem letter (no `FULLSTRIP`).
        let base = word.strip_suffix(r.strip.as_str()).filter(|b| !b.is_empty())?;
        Some(format!("{base}{}", r.add))
    }

    fn prefix(word: &str, r: &Rule) -> Option<String> {
        let mut cs = word.chars();
        if !r.cond.iter().all(|set| cs.next().is_some_and(|c| set.has(c))) {
            return None;
        }
        let base = word.strip_prefix(r.strip.as_str()).filter(|b| !b.is_empty())?;
        Some(format!("{}{base}", r.add))
    }
}

/// Flags as written, or the `AF` alias a number names (1-based).
fn resolve(aliases: &[Vec<String>], mode: FlagMode, s: &str) -> Vec<String> {
    match s.parse::<usize>().ok().and_then(|n| n.checked_sub(1)).and_then(|i| aliases.get(i)) {
        Some(a) => a.clone(),
        None => split_flags(mode, s),
    }
}

fn split_flags(mode: FlagMode, s: &str) -> Vec<String> {
    match mode {
        FlagMode::Char => s.chars().map(String::from).collect(),
        FlagMode::Long => s.chars().collect::<Vec<_>>().chunks(2).map(|c| c.iter().collect()).collect(),
        FlagMode::Num => s.split(',').filter(|f| !f.is_empty()).map(str::to_string).collect(),
    }
}

/// A flag as a feature name: letters and digits kept, anything else by code.
fn feature(kind: &str, flag: &str) -> String {
    let name: String = flag.chars().map(|c| if c.is_alphanumeric() { c.to_string() } else { format!("u{:X}", c as u32) }).collect();
    format!("{kind}_{name}")
}

/// UniMorph rows (`lemma<TAB>form<TAB>features`) for every form the
/// dictionary spells.
pub fn to_unimorph(aff: &str, dic: &str) -> Result<String> {
    let (aff, dic) = (aff.strip_prefix('\u{FEFF}').unwrap_or(aff), dic.strip_prefix('\u{FEFF}').unwrap_or(dic));
    let a = Aff::parse(aff)?;
    let mut rows: BTreeSet<String> = BTreeSet::new();
    for (i, raw) in dic.lines().enumerate() {
        let line = raw.trim_end();
        if line.is_empty() || (i == 0 && line.chars().all(|c| c.is_ascii_digit())) {
            continue;
        }
        let (entry, morph) = line.split_once(['\t', ' ']).unwrap_or((line, ""));
        let (word, flags) = split_entry(entry);
        if word.is_empty() || word.contains(char::is_whitespace) {
            continue;
        }
        let flags = flags.map(|f| a.flags(f)).unwrap_or_default();
        if a.forbidden.as_ref().is_some_and(|f| flags.contains(f)) {
            continue;
        }
        let pos = morph.split_whitespace().find_map(|f| f.strip_prefix("po:")).map(sanitize).unwrap_or_else(|| "STEM".to_string());
        if !a.need_affix.as_ref().is_some_and(|f| flags.contains(f)) {
            rows.insert(format!("{word}\t{word}\t{pos}"));
        }
        expand(&a, &word, &pos, &flags, &mut rows);
    }
    Ok(rows.into_iter().map(|r| r + "\n").collect())
}

/// The affix classes among `flags`, prefixes or suffixes.
fn classes<'a>(a: &'a Aff, flags: &'a [String], prefix: bool) -> impl Iterator<Item = (&'a String, &'a Class)> {
    flags.iter().filter_map(move |f| a.classes.get(f).filter(|c| c.prefix == prefix).map(|c| (f, c)))
}

/// Suffixed forms of `word`: (form, features, may take a prefix too).
fn suffixed(a: &Aff, word: &str, flags: &[String]) -> Vec<(String, Vec<String>, bool)> {
    let mut out = Vec::new();
    for (f, c) in classes(a, flags, false) {
        for (form, r) in c.rules.iter().filter_map(|r| Some((Aff::suffix(word, r)?, r))) {
            let first = feature("SFX", f);
            for (g, c2) in classes(a, &r.cont, false) {
                let more = c2.rules.iter().filter_map(|r2| Aff::suffix(&form, r2));
                out.extend(more.map(|f2| (f2, vec![first.clone(), feature("SFX", g)], c.cross && c2.cross)));
            }
            out.push((form, vec![first], c.cross));
        }
    }
    out
}

fn expand(a: &Aff, word: &str, pos: &str, flags: &[String], rows: &mut BTreeSet<String>) {
    let suffixed = suffixed(a, word, flags);
    for (form, feats, _) in &suffixed {
        rows.insert(format!("{word}\t{form}\t{pos};{}", feats.join(";")));
    }
    for (f, c) in classes(a, flags, true) {
        let pfx = feature("PFX", f);
        for r in &c.rules {
            if let Some(form) = Aff::prefix(word, r) {
                rows.insert(format!("{word}\t{form}\t{pos};{pfx}"));
            }
            let crossed = suffixed.iter().filter(|s| c.cross && s.2).filter_map(|(s, feats, _)| Some((Aff::prefix(s, r)?, feats)));
            for (form, feats) in crossed {
                rows.insert(format!("{word}\t{form}\t{pos};{pfx};{}", feats.join(";")));
            }
        }
    }
}

/// `word/flags`, where `\/` is a slash inside the word.
fn split_entry(entry: &str) -> (String, Option<&str>) {
    let mut word = String::new();
    let mut cs = entry.char_indices().peekable();
    while let Some((i, c)) = cs.next() {
        match c {
            '\\' if cs.peek().is_some_and(|&(_, n)| n == '/') => {
                word.push('/');
                cs.next();
            }
            '/' => return (word, entry.get(i + 1..)),
            _ => word.push(c),
        }
    }
    (word, None)
}

/// A `po:` value as a UniMorph part of speech where one fits.
fn sanitize(s: &str) -> String {
    let t: String = s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_uppercase();
    match t.as_str() {
        "" => "STEM",
        "NOUN" | "NOM" | "SUBST" | "N" => "N",
        "VERB" | "VERBE" | "V" => "V",
        "ADJ" | "ADJECTIVE" | "ADJECTIF" => "ADJ",
        "ADV" | "ADVERB" | "ADVERBE" => "ADV",
        "PRON" | "PRONOUN" | "PRONOM" => "PRO",
        _ => return t,
    }
    .to_string()
}
