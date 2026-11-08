//! Line-oriented parser for `.morph` files. `!` starts a comment.

use super::ast::*;
use crate::error::{spec_err, Result};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    None,
    Lexicon,
    Rules,
    Pieces,
    Feature,
    Archi,
}

/// Is `OPTION <name> true` set? Read before parsing, since it changes the
/// text that is parsed.
fn option_on(text: &str, name: &str) -> bool {
    text.contains(name)
        && text.lines().any(|l| {
            let t: Vec<&str> = l.split('!').next().unwrap_or("").split_whitespace().collect();
            t == ["OPTION", name, "true"]
        })
}

/// The spec text as the lexer reads input: Hangul in jamo, so morpheme
/// boundaries may fall inside a syllable (가 + ᆻ), and without vowel marks
/// when input drops them.
fn prepare(text: &str) -> std::borrow::Cow<'_, str> {
    let mut out = std::borrow::Cow::Borrowed(text);
    if option_on(text, "hangul_jamo") {
        out = crate::normalize::decompose_hangul(&out).into();
    }
    if option_on(text, "strip_vowel_marks") {
        out = crate::normalize::strip_vowel_marks(&out).into();
    }
    out
}

pub fn parse_spec(text: &str) -> Result<Spec> {
    let prepared = prepare(text);
    let text = prepared.as_ref();

    let mut spec = Spec::default();
    let mut section = Section::None;
    for (idx, raw) in text.lines().enumerate() {
        let line = idx + 1;
        let body = match raw.find('!') {
            Some(i) => &raw[..i],
            None => raw,
        }
        .trim();
        if body.is_empty() {
            continue;
        }
        let toks: Vec<&str> = body.split_whitespace().collect();
        match toks[0] {
            "OPTION" => parse_option(&mut spec.options, &toks, line)?,
            "INDUCE" => {
                let Some(k) = toks.iter().position(|t| *t == "IN") else {
                    return spec_err(line, "expected `INDUCE feature.. IN Lexicon..`");
                };
                if k < 2 || k + 1 >= toks.len() {
                    return spec_err(line, "expected `INDUCE feature.. IN Lexicon..`");
                }
                spec.induce.push(InduceDef {
                    features: toks[1..k].iter().map(|s| s.to_string()).collect(),
                    lexicons: toks[k + 1..].iter().map(|s| s.to_string()).collect(),
                    line,
                });
            }
            "CLASS" if toks.get(2) == Some(&"=") => spec.classes.push(parse_class(&toks, line)?),
            // An entry ends in `;` and a piece list is not a rule, so `PREFER` stays spellable.
            "PREFER" if section != Section::Pieces && !body.ends_with(';') => spec.options.context.push(parse_prefer(&toks, line)?),
            "LEXICON" => {
                if toks.len() < 2 || toks.len() > 3 {
                    return spec_err(line, "expected `LEXICON Name [@open=TAG]`");
                }
                let name = toks[1].to_string();
                if name == "#" || name == "0" {
                    return spec_err(line, format!("`{name}` is reserved"));
                }
                let open_tag = match toks.get(2) {
                    None => None,
                    Some(t) => match t.strip_prefix("@open=") {
                        Some(tag) if !tag.is_empty() => Some(tag.to_string()),
                        _ => return spec_err(line, format!("unknown lexicon flag `{t}`")),
                    },
                };
                spec.lexicons.push(SpecLexicon { name, open_tag, entries: Vec::new(), line });
                section = Section::Lexicon;
            }
            "FEATURE" | "RFEATURE" if toks.len() == 2 => {
                spec.features.push(FeatureDef { name: toks[1].to_string(), lines: vec![], right: toks[0] == "RFEATURE", line });
                section = Section::Feature;
            }
            "ARCHI" if toks.len() == 2 => {
                spec.archis.push(ArchiDef { name: toks[1].to_string(), lines: vec![], line });
                section = Section::Archi;
            }
            "RULES" if toks.len() == 1 => section = Section::Rules,
            "PIECES" if toks.len() == 1 => section = Section::Pieces,
            _ => match section {
                Section::Lexicon => {
                    let e = parse_entry(body, line)?;
                    let Some(lex) = spec.lexicons.last_mut() else { return spec_err(line, "entry outside a LEXICON") };
                    lex.entries.push(e);
                }
                Section::Rules => spec.rules.push(parse_rule(body, line)?),
                Section::Pieces => spec.pieces.extend(toks.iter().map(|t| (t.to_string(), line))),
                Section::Feature => {
                    let l = parse_feat_line(body, line)?;
                    let Some(feat) = spec.features.last_mut() else { return spec_err(line, "line outside a FEATURE") };
                    feat.lines.push(l);
                }
                Section::Archi => {
                    let l = parse_archi_line(body, line)?;
                    let Some(archi) = spec.archis.last_mut() else { return spec_err(line, "line outside an ARCHI") };
                    archi.lines.push(l);
                }
                Section::None => return spec_err(line, "content before any LEXICON/RULES/PIECES section"),
            },
        }
    }
    Ok(spec)
}

/// `PREFER N,ADJ [OVER ADV] AFTER|BEFORE DET,"to"`.
fn parse_prefer(toks: &[&str], line: usize) -> Result<ContextRule> {
    let usage = "expected `PREFER tags [OVER tags] AFTER|BEFORE tags`, tags comma-separated, a lemma in quotes";
    let sels = |t: &str| -> Result<Vec<Sel>> {
        t.split(',')
            .map(|s| match s.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                Some(l) if !l.is_empty() => Ok(Sel::Lemma(l.to_string())),
                None if !s.is_empty() && !s.contains('"') => Ok(Sel::Tag(s.to_string())),
                _ => spec_err(line, usage),
            })
            .collect()
    };
    let (prefer, over, side, near) = match toks {
        [_, p, "OVER", o, side, n] => (sels(p)?, sels(o)?, *side, sels(n)?),
        [_, p, side, n] => (sels(p)?, Vec::new(), *side, sels(n)?),
        _ => return spec_err(line, usage),
    };
    let before = match side {
        "AFTER" => false,
        "BEFORE" => true,
        _ => return spec_err(line, usage),
    };
    Ok(ContextRule { prefer, over, before, near })
}

fn parse_option(o: &mut Options, toks: &[&str], line: usize) -> Result<()> {
    if toks.len() != 3 {
        return spec_err(line, "expected `OPTION key value`");
    }
    let boolean = |v: &str| match v {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => spec_err(line, "expected true/false"),
    };
    match toks[1] {
        "max_morph_chars" => match toks[2].parse::<usize>() {
            Ok(v) if v > 0 && v <= 4096 => o.max_morph_chars = v,
            _ => return spec_err(line, "max_morph_chars must be in 1..=4096"),
        },
        "suffix_initial_pieces" => o.suffix_initial_pieces = boolean(toks[2])?,
        "hangul_jamo" => o.hangul_jamo = boolean(toks[2])?,
        "apostrophe_in_words" => o.apostrophe_in_words = boolean(toks[2])?,
        "strip_tones" => o.strip_tones = boolean(toks[2])?,
        "strip_vowel_marks" => o.strip_vowel_marks = boolean(toks[2])?,
        "harmony" => {
            o.underlying = match toks[2] {
                "surface" => false,
                "underlying" => true,
                _ => return spec_err(line, "harmony must be surface or underlying"),
            }
        }
        "derivation" => {
            let tags: Vec<String> = toks[2].split(',').map(str::to_string).collect();
            if tags.iter().any(String::is_empty) {
                return spec_err(line, "derivation takes comma-separated tags, e.g. PFX,NMLZ");
            }
            o.derivation = tags;
        }
        "induce_min_count" => match toks[2].parse::<usize>() {
            Ok(v) if v >= 1 => o.induce_min_count = v,
            _ => return spec_err(line, "induce_min_count must be ≥ 1"),
        },
        "case" => {
            o.case = match toks[2] {
                "lower" => CaseMode::Lower,
                "turkish" => CaseMode::Turkish,
                "none" => CaseMode::None,
                _ => return spec_err(line, "case must be lower, turkish or none"),
            }
        }
        k => return spec_err(line, format!("unknown option `{k}`")),
    }
    Ok(())
}

/// `CLASS NAME = a b c {A}` (multi-char tokens contribute each char).
fn parse_class(toks: &[&str], line: usize) -> Result<ClassDef> {
    let name = toks[1].to_string();
    if name.contains(['[', ']', '{', '}', '*']) {
        return spec_err(line, "bad class name");
    }
    let mut members = Vec::new();
    for t in &toks[3..] {
        let syms = parse_template(t).map_err(|m| crate::Error::Spec { line, msg: m })?;
        members.extend(syms);
    }
    if members.is_empty() {
        return spec_err(line, format!("class `{name}` is empty"));
    }
    Ok(ClassDef { name, members, line })
}

fn strip_semicolon(body: &str, line: usize) -> Result<&str> {
    match body.strip_suffix(';') {
        Some(b) => Ok(b.trim_end()),
        None => spec_err(line, "entry must end with `;`"),
    }
}

fn parse_entry(body: &str, line: usize) -> Result<SpecEntry> {
    let body = strip_semicolon(body, line)?;
    let toks: Vec<&str> = body.split_whitespace().collect();
    match toks.len() {
        2 => {
            if toks[0] != "0" {
                return spec_err(line, "two-field entries must be epsilon: `0 Cont ;`");
            }
            Ok(SpecEntry {
                surface: String::new(),
                lemma: String::new(),
                tag: String::new(),
                cont: toks[1].to_string(),
                flags: vec![],
                zeros: vec![],
                line,
            })
        }
        n if n >= 3 && toks[0] == "0" => {
            if n > 3 {
                return spec_err(line, "a zero morpheme is `0 TAG Cont ;` (no flags, no extra fields)");
            }
            Ok(SpecEntry {
                surface: String::new(),
                lemma: String::new(),
                tag: toks[1].to_string(),
                cont: toks[2].to_string(),
                flags: vec![],
                zeros: vec![],
                line,
            })
        }
        n if n >= 3 => {
            let (surface, lemma) = match toks[0].split_once(':') {
                Some((s, l)) => (s.to_string(), l.to_string()),
                None => (toks[0].to_string(), strip_braces(toks[0])),
            };
            if surface == "0" || surface.is_empty() || lemma.is_empty() {
                return spec_err(line, "non-epsilon entry needs a non-empty surface and lemma (epsilon is `0 Cont ;`)");
            }
            if surface.contains('#') {
                return spec_err(line, "`#` is not allowed inside morpheme surfaces");
            }
            parse_template(&surface).map_err(|m| crate::Error::Spec { line, msg: m })?;
            let mut flags = Vec::new();
            for f in &toks[3..] {
                match f.strip_prefix('@') {
                    Some(name) if !name.is_empty() => flags.push(name.to_string()),
                    _ => return spec_err(line, format!("unexpected token `{f}` (flags start with @)")),
                }
            }
            Ok(SpecEntry { surface, lemma, tag: toks[1].to_string(), cont: toks[2].to_string(), flags, zeros: vec![], line })
        }
        _ => spec_err(line, "expected `surface[:lemma] TAG Cont [@flag..] ;`, `0 TAG Cont ;` or `0 Cont ;`"),
    }
}

/// `[NAME]`, `[*]`, with optional quantifier; or literal chars.
fn parse_atoms(tok: &str, line: usize) -> Result<Vec<(Atom, Quant)>> {
    if let Some(rest) = tok.strip_prefix('[') {
        let (inner, q) = match rest.rsplit_once(']') {
            Some((i, q)) => (i, q),
            None => return spec_err(line, format!("unterminated `[` in `{tok}`")),
        };
        let quant = match q {
            "" => Quant::One,
            "+" => Quant::Plus,
            "*" => Quant::Star,
            "?" => Quant::Opt,
            _ => return spec_err(line, format!("bad quantifier `{q}`")),
        };
        let atom = if inner == "*" { Atom::Any } else { Atom::Class(inner.to_string()) };
        if inner.is_empty() {
            return spec_err(line, "empty `[]`");
        }
        return Ok(vec![(atom, quant)]);
    }
    Ok(tok.chars().map(|c| (Atom::Ch(c), Quant::One)).collect())
}

fn parse_cond_tok(t: &str, line: usize) -> Result<(String, Vec<String>)> {
    match t.split_once('=') {
        Some((f, vs)) if !f.is_empty() && !vs.is_empty() => {
            Ok((f.to_string(), vs.split('|').map(String::from).collect()))
        }
        _ => spec_err(line, format!("expected `feature=value|value`, got `{t}`")),
    }
}

fn parse_feat_line(body: &str, line: usize) -> Result<FeatLine> {
    let body = body.strip_suffix(';').unwrap_or(body).trim();
    let (cond, rest) = if let Some(r) = body.strip_prefix('<') {
        let Some((c, rest)) = r.split_once('>') else { return spec_err(line, "unterminated `<`") };
        let cond = c.split_whitespace().map(|t| parse_cond_tok(t, line)).collect::<Result<Vec<_>>>()?;
        (cond, rest)
    } else {
        (vec![], body)
    };
    let toks: Vec<&str> = rest.split_whitespace().collect();
    let Some(arrow) = toks.iter().position(|t| *t == "->") else {
        return spec_err(line, "expected `atoms -> value`");
    };
    if arrow == 0 || arrow + 2 != toks.len() {
        return spec_err(line, "expected `atoms -> value`");
    }
    let mut atoms = Vec::new();
    for t in &toks[..arrow] {
        for (a, q) in parse_atoms(t, line)? {
            if q != Quant::One {
                return spec_err(line, "quantifiers are not allowed in FEATURE lines");
            }
            atoms.push(a);
        }
    }
    let value = toks[arrow + 1].to_string();
    if value == "none" {
        return spec_err(line, "`none` is the implicit initial value and cannot be assigned");
    }
    Ok(FeatLine { cond, atoms, value, line })
}

fn parse_archi_line(body: &str, line: usize) -> Result<ArchiLine> {
    let body = body.strip_suffix(';').unwrap_or(body).trim();
    let toks: Vec<&str> = body.split_whitespace().collect();
    let Some(arrow) = toks.iter().position(|t| *t == "->") else {
        return spec_err(line, "expected `cond -> output`");
    };
    if arrow == 0 || arrow + 2 != toks.len() {
        return spec_err(line, "expected `cond -> output`");
    }
    let cond = if toks[..arrow] == ["*"] {
        None
    } else {
        Some(toks[..arrow].iter().map(|t| parse_cond_tok(t, line)).collect::<Result<Vec<_>>>()?)
    };
    let out = toks[arrow + 1];
    let output = if out == "0" { String::new() } else { out.to_string() };
    if output.contains(['{', '}']) {
        return spec_err(line, "archiphoneme outputs must be literal text");
    }
    Ok(ArchiLine { cond, output, line })
}

/// Split `a,{S},b` at commas outside braces.
fn split_set(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '{' => {
                depth += 1;
                cur.push(c)
            }
            '}' => {
                depth -= 1;
                cur.push(c)
            }
            ',' if depth == 0 => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out.into_iter().map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
}

fn parse_rule(body: &str, line: usize) -> Result<SpecRule> {
    let body = strip_semicolon(body, line)?;
    let toks: Vec<&str> = body.split_whitespace().collect();
    let bad = || spec_err(line, "expected `name[?]: TARGET -> REPL / [LEFT] _ [TAIL..] +{a,b}|+[CLASS] [@flag] ;`");
    if toks.len() < 7 {
        return bad();
    }
    let Some(head) = toks[0].strip_suffix(':') else { return bad() };
    let (name, optional) = match head.strip_suffix('?') {
        Some(n) => (n, true),
        None => (head, false),
    };
    if name.is_empty() || toks[2] != "->" || toks[4] != "/" {
        return bad();
    }
    let target = match toks[1] {
        "." => Target::AnyChar,
        "0" => return spec_err(line, "rule target cannot be empty"),
        t if t.starts_with('[') && t.ends_with(']') && t.len() > 2 => Target::Class(t[1..t.len() - 1].to_string()),
        t => Target::Literal(t.to_string()),
    };
    let replacement = if toks[3] == "0" { String::new() } else { toks[3].to_string() };
    let mut i = 5;
    let left = if toks[i] == "_" {
        LeftCtx::Any
    } else {
        let l = match toks[i] {
            "C" => LeftCtx::Consonant,
            "V" => LeftCtx::Vowel,
            "^" => LeftCtx::Boundary,
            t if t.starts_with('[') && t.ends_with(']') && t.len() > 2 => LeftCtx::Class(t[1..t.len() - 1].to_string()),
            t if t.chars().all(char::is_alphabetic) => LeftCtx::Literal(t.to_string()),
            t => return spec_err(line, format!("unknown left context `{t}` (use C, V, ^, [CLASS] or letters)")),
        };
        i += 1;
        if toks.get(i) != Some(&"_") {
            return bad();
        }
        l
    };
    i += 1;
    let mut tail = Vec::new();
    while let Some(t) = toks.get(i) {
        if t.starts_with('+') {
            break;
        }
        tail.extend(parse_atoms(t, line)?);
        i += 1;
    }
    let Some(rt) = toks.get(i) else { return bad() };
    let right = if let Some(set) = rt.strip_prefix("+{").and_then(|t| t.strip_suffix('}')) {
        let v = split_set(set);
        if v.is_empty() {
            return spec_err(line, "rule right context is empty");
        }
        RightCtx::Lex(v)
    } else if let Some(c) = rt.strip_prefix("+[").and_then(|t| t.strip_suffix(']')) {
        if c.is_empty() {
            return bad();
        }
        RightCtx::Class(c.to_string())
    } else {
        return bad();
    };
    i += 1;
    let require_flag = match toks.get(i) {
        None => None,
        Some(f) => match f.strip_prefix('@') {
            Some(n) if !n.is_empty() && i + 1 == toks.len() => Some(n.to_string()),
            _ => return bad(),
        },
    };
    Ok(SpecRule { name: name.to_string(), optional, target, replacement, left, tail, right, require_flag, line })
}
