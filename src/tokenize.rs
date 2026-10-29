//! Lossless tokenizer: running text in, byte spans and classes out.
//!
//! Every non-space character belongs to exactly one token, and only
//! whitespace lies between tokens, so the text can be rebuilt from the spans.
//! Rules are tried in a fixed order, longest-first, with no tuned weights:
//! URL, email, initialism (`U.S.`), then word / number, symbol, punctuation.
//! English clitics split off (`Smith|'s`, `do|n't`); other inner apostrophes
//! stay (`o'clock`). Spans index the original text, so normalizing a token
//! later cannot shift them.

use crate::pretok::is_punct;
use std::ops::Range;

/// What a token is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Class {
    /// Letters (and marks, emoji joiners): the only class a lexicon is asked about.
    Word,
    /// Contains a digit: `4.50`, `1,000`, `3rd`, `r2d2`.
    Number,
    /// `https://…` or `www.…`, without trailing punctuation.
    Url,
    /// `name@host.tld`.
    Email,
    /// `Dr.`, `etc.`, `e.g.`, `U.S.`: the period belongs to the token.
    Abbreviation,
    /// `'s`, `n't`, `'re`, `'ve`, `'ll`, `'d`, `'m` after a word.
    Clitic,
    /// A punctuation mark, or a run of one repeated mark (`...`, `--`).
    Punct,
    /// Currency, math signs, emoji.
    Symbol,
}

impl Class {
    /// The tag a non-word token carries in an analysis.
    pub(crate) fn tag(self) -> &'static str {
        match self {
            Class::Word => "WORD",
            Class::Number => "NUM",
            Class::Url => "URL",
            Class::Email => "EMAIL",
            Class::Abbreviation => "ABBR",
            Class::Clitic => "CLITIC",
            Class::Punct => "PUNCT",
            Class::Symbol => "SYM",
        }
    }
}

/// One token: where it sits in the text and what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    span: Range<usize>,
    class: Class,
}

impl Token {
    /// Byte range in the text that was tokenized.
    pub fn span(&self) -> Range<usize> {
        self.span.clone()
    }

    pub fn class(&self) -> Class {
        self.class
    }
}

/// `Texas`: an uppercase letter, then only lowercase letters.
pub(crate) fn is_capitalized(surface: &str) -> bool {
    let mut chars = surface.chars();
    chars.next().is_some_and(char::is_uppercase) && chars.clone().next().is_some() && chars.all(char::is_lowercase)
}

/// Whether token `i` opens a sentence: first token, after `.`/`!`/`?`, or
/// on a new line. Opening quotes and brackets in between are skipped.
pub(crate) fn starts_sentence(text: &str, tokens: &[Token], i: usize) -> bool {
    let Some(at) = tokens.get(i).map(|t| t.span.start) else { return true };
    let mut k = i;
    while k > 0 {
        k -= 1;
        let Some(prev) = tokens.get(k) else { return true };
        let surface = text.get(prev.span()).unwrap_or_default();
        let gap = text.get(prev.span.end..at).unwrap_or_default();
        if gap.contains('\n') {
            return true;
        }
        let opener = prev.class == Class::Punct && surface.chars().all(|c| "\"'“‘([{«".contains(c));
        if !opener {
            return prev.class == Class::Punct && surface.ends_with(['.', '!', '?', '…']);
        }
    }
    true
}

const ABBREVIATIONS: [&str; 30] = [
    "dr", "mr", "mrs", "ms", "prof", "sr", "jr", "st", "vs", "etc", "inc", "ltd", "co", "corp", "mt", "gen", "gov", "sen", "rep", "lt", "col", "capt",
    "sgt", "rev", "fig", "approx", "dept", "est", "vol", "cf",
];
const CLITICS: [&str; 6] = ["s", "re", "ve", "ll", "d", "m"];

/// Split `text` into tokens, with English-style apostrophes.
pub fn tokenize(text: &str) -> Vec<Token> {
    scan(text, false)
}

/// `apostrophe_in_words`: `'` is a letter (`l'homme`), so nothing splits at it.
pub(crate) fn scan(text: &str, apostrophe_in_words: bool) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::new();
    let mut at = 0;
    while let Some(rest) = text.get(at..) {
        let Some(c) = rest.chars().next() else { break };
        if c.is_whitespace() {
            at += c.len_utf8();
            continue;
        }
        let after_word = out.last().is_some_and(|t| t.span.end == at && t.class == Class::Word);
        let (len, class) = next(rest, after_word, apostrophe_in_words);
        let len = len.max(c.len_utf8());
        out.push(Token { span: at..at + len, class });
        at += len;
    }
    out
}

fn next(rest: &str, after_word: bool, apostrophe_in_words: bool) -> (usize, Class) {
    if let Some(n) = url_len(rest) {
        return (n, Class::Url);
    }
    if let Some(n) = email_len(rest) {
        return (n, Class::Email);
    }
    if let Some(n) = initialism_len(rest) {
        return (n, Class::Abbreviation);
    }
    if after_word && !apostrophe_in_words {
        if let Some(n) = clitic_len(rest) {
            return (n, Class::Clitic);
        }
    }
    let Some(c) = rest.chars().next() else { return (0, Class::Punct) };
    if is_symbol(c) {
        return (symbol_len(rest), Class::Symbol);
    }
    if is_punct(c) && !(apostrophe_in_words && is_apostrophe(c)) {
        let run = rest.chars().take_while(|&d| d == c).count();
        let repeats = run > 1 && ".!?-*=_~".contains(c);
        return (if repeats { run * c.len_utf8() } else { c.len_utf8() }, Class::Punct);
    }
    word_len(rest, apostrophe_in_words)
}

fn is_apostrophe(c: char) -> bool {
    c == '\'' || c == '\u{2019}'
}

fn is_symbol(c: char) -> bool {
    "$%&+<=>^`|~".contains(c)
        || matches!(c,
            '\u{00A2}'..='\u{00A6}' | '\u{00A9}' | '\u{00AE}' | '\u{00B0}' | '\u{00B1}' | '\u{00D7}' | '\u{00F7}'
            | '\u{20A0}'..='\u{20CF}' | '\u{2190}'..='\u{23FF}' | '\u{2600}'..='\u{27BF}' | '\u{2B00}'..='\u{2BFF}'
            | '\u{1F000}'..='\u{1FAFF}')
}

/// A symbol plus the marks that make an emoji one glyph: variation selector,
/// skin tone, and `ZWJ` + symbol sequences.
fn symbol_len(rest: &str) -> usize {
    let mut chars = rest.char_indices().peekable();
    let mut end = chars.next().map_or(0, |(_, c)| c.len_utf8());
    while let Some(&(i, c)) = chars.peek() {
        let joins = c == '\u{FE0F}' || ('\u{1F3FB}'..='\u{1F3FF}').contains(&c);
        if joins {
            end = i + c.len_utf8();
            chars.next();
        } else if c == '\u{200D}' {
            chars.next();
            match chars.peek() {
                Some(&(j, d)) if is_symbol(d) => {
                    end = j + d.len_utf8();
                    chars.next();
                }
                _ => break,
            }
        } else {
            break;
        }
    }
    end
}

/// A run of word characters: letters, digits, marks, and the joiners that
/// belong inside a word (`4.50`, `1,000`, `o'clock`). Stops before an English
/// clitic so `Smith's` is `Smith` + `'s`.
fn word_len(rest: &str, apostrophe_in_words: bool) -> (usize, Class) {
    let chars: Vec<(usize, char)> = rest.char_indices().collect();
    let at = |k: usize| chars.get(k).map(|&(_, c)| c);
    let is_word = |c: char| !c.is_whitespace() && !is_punct(c) && !is_symbol(c);
    let mut k = 0;
    while let Some(c) = at(k) {
        let prev = k.checked_sub(1).and_then(at);
        let next = at(k + 1);
        let inner = prev.is_some_and(char::is_alphanumeric) && next.is_some_and(char::is_alphanumeric);
        let joins = if is_apostrophe(c) {
            apostrophe_in_words || (inner && !is_clitic_tail(&chars, k + 1))
        } else {
            matches!(c, '.' | ',') && prev.is_some_and(|p| p.is_ascii_digit()) && next.is_some_and(|n| n.is_ascii_digit())
        };
        if !(is_word(c) || joins) {
            break;
        }
        k += 1;
    }
    let mut end = chars.get(k).map_or(rest.len(), |&(i, _)| i);
    let core = rest.get(..end).unwrap_or_default();
    if !apostrophe_in_words && at(k).is_some_and(is_apostrophe) && core.chars().count() > 1 && core.ends_with(['n', 'N']) && tail_is(&chars, k + 1, "t") {
        end -= 1;
    }
    let core = rest.get(..end).unwrap_or_default();
    if core.chars().all(char::is_alphabetic) && rest.get(end..).is_some_and(|r| r.starts_with('.')) && ABBREVIATIONS.contains(&core.to_lowercase().as_str()) {
        return (end + 1, Class::Abbreviation);
    }
    let class = if core.chars().any(char::is_numeric) { Class::Number } else { Class::Word };
    (end, class)
}

/// The alphanumeric run starting at char index `from`, lowercased.
fn tail(chars: &[(usize, char)], from: usize) -> String {
    chars.iter().skip(from).map(|&(_, c)| c).take_while(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn tail_is(chars: &[(usize, char)], from: usize, want: &str) -> bool {
    tail(chars, from) == want
}

fn is_clitic_tail(chars: &[(usize, char)], from: usize) -> bool {
    let t = tail(chars, from);
    CLITICS.contains(&t.as_str()) || t == "t"
}

/// `'s`, `n't` and friends, only reached right after a word.
fn clitic_len(rest: &str) -> Option<usize> {
    let chars: Vec<(usize, char)> = rest.char_indices().collect();
    let first = chars.first().map(|&(_, c)| c)?;
    let (apostrophe_at, want_t) = if is_apostrophe(first) {
        (0, false)
    } else if matches!(first, 'n' | 'N') && chars.get(1).is_some_and(|&(_, c)| is_apostrophe(c)) {
        (1, true)
    } else {
        return None;
    };
    let t = tail(&chars, apostrophe_at + 1);
    let ok = if want_t { t == "t" } else { CLITICS.contains(&t.as_str()) };
    ok.then(|| rest.char_indices().nth(apostrophe_at + 1 + t.chars().count()).map_or(rest.len(), |(i, _)| i))
}

/// `U.S.`, `e.g.`: at least two letter-period pairs.
fn initialism_len(rest: &str) -> Option<usize> {
    let mut pairs = 0;
    let mut end = 0;
    let mut it = rest.char_indices();
    while let (Some((_, a)), Some((j, '.'))) = (it.next(), it.next()) {
        if !a.is_alphabetic() {
            break;
        }
        pairs += 1;
        end = j + 1;
    }
    let boundary = rest.get(end..).and_then(|r| r.chars().next()).is_none_or(|c| !c.is_alphanumeric());
    (pairs >= 2 && boundary).then_some(end)
}

fn url_len(rest: &str) -> Option<usize> {
    let prefix = ["http://", "https://", "ftp://", "www."].into_iter().find(|p| rest.get(..p.len()).is_some_and(|s| s.eq_ignore_ascii_case(p)))?;
    let run = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let url = rest.get(..run)?.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}', '\'', '"']);
    (url.len() > prefix.len()).then_some(url.len())
}

fn email_len(rest: &str) -> Option<usize> {
    let run = rest.find(|c: char| !(c.is_alphanumeric() || "._%+-@".contains(c))).unwrap_or(rest.len());
    let (local, domain) = rest.get(..run)?.split_once('@')?;
    if local.is_empty() || domain.contains('@') {
        return None;
    }
    let domain = domain.trim_end_matches(['.', '-', '_', '%', '+']);
    let dot = domain.rfind('.')?;
    let tld = domain.get(dot + 1..)?;
    (dot > 0 && tld.chars().count() >= 2 && tld.chars().all(char::is_alphabetic)).then_some(local.len() + 1 + domain.len())
}
