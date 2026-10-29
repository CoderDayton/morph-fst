//! Command-line front end. Run `morph-fst help` for usage.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unreachable, clippy::todo, clippy::unimplemented)]

use std::io::Read;
use std::process::ExitCode;
use std::time::Instant;

use morph_fst::fused::derive_vocab;
use morph_fst::output::{analysis_string, segmentation, WordOutput};
use morph_fst::rng::Rng;
use morph_fst::normalize::compose_hangul;
use morph_fst::{oracle, spec, Error, Lexer, Mode, Result};

const USAGE: &str = "\
morph-fst — morphological FST with deterministic subword fallback

NEW LANGUAGE (the easy way)
  new <code> -o <lexer.mcfu> [--wordlist <words.txt>] [--corpus <text.txt>]
         downloads the language's UniMorph table (<code> is its three-letter
         code: rus, spa, tur, ...) and builds a lexer. --unimorph <file> or
         --spec <file> replace the download; --wordlist covers words the
         table misses (and alone, builds from the list only).

BUILD (one source, one compile loop, one artifact)
  build  <spec.morph> -o <lexer.mcfu> [--corpus <text.txt>]
         (--corpus: learn the spec's INDUCE exceptions from raw text first)
  induce <spec.morph> <text.txt> [-o <annotated.morph>]
         show which roots are exceptions; -o writes them into the spec
  generate <unimorph.tsv> [-o <spec.morph>]
         draft a spec from UniMorph tables (lemma<TAB>form<TAB>features)

USE
  analyze  <SOURCE> word...                     all morphological analyses
  tokenize <SOURCE> [--mode strict|hybrid] [--tokens] [--keep-case] [text...]
           (reads stdin when no text is given)
  dump-expanded <spec.morph>                    show rule-expanded entries
  bench <spec.morph> [--words N] [--reps R]

SOURCE is one of:  --spec <spec.morph>  |  --fused <lexer.mcfu>  |  --unimorph <unimorph.tsv>
                   |  --wordlist <words.txt>
(--unimorph drafts a spec from tables, --wordlist from a bare word list: no spec to write.)
A <spec.morph> argument may also name a bundled language:
  english, turkish, spanish, french, korean, igbo
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn read(path: &str) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| Error::Io(format!("{path}: {e}")))
}

fn read_text(path: &str) -> Result<String> {
    String::from_utf8(read(path)?).map_err(|_| Error::Io(format!("{path}: not UTF-8")))
}

/// A spec file, or the name of a bundled language.
fn spec_text(arg: &str) -> Result<String> {
    if !std::path::Path::new(arg).exists() {
        if let Some(src) = morph_fst::languages::by_name(arg) {
            return Ok(src.to_string());
        }
    }
    read_text(arg)
}

fn write(path: &str, data: &[u8]) -> Result<()> {
    std::fs::write(path, data).map_err(|e| Error::Io(format!("{path}: {e}")))
}

/// Tiny flag parser: returns (positional, flags-with-values, bare switches).
struct Args {
    pos: Vec<String>,
    kv: Vec<(String, String)>,
    sw: Vec<String>,
}

impl Args {
    fn parse(a: &[String]) -> Result<Args> {
        const WITH_VALUE: &[&str] = &["-o", "--spec", "--fused", "--mode", "--words", "--reps", "--corpus", "--unimorph", "--wordlist"];
        let mut out = Args { pos: vec![], kv: vec![], sw: vec![] };
        let mut i = 0;
        while i < a.len() {
            let x = &a[i];
            if WITH_VALUE.contains(&x.as_str()) {
                let v = a.get(i + 1).ok_or_else(|| Error::Io(format!("{x} needs a value")))?;
                out.kv.push((x.clone(), v.clone()));
                i += 2;
            } else if x.starts_with("--") {
                out.sw.push(x.clone());
                i += 1;
            } else {
                out.pos.push(x.clone());
                i += 1;
            }
        }
        Ok(out)
    }
    fn get(&self, k: &str) -> Option<&str> {
        self.kv.iter().rev().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
    }
    fn has(&self, k: &str) -> bool {
        self.sw.iter().any(|s| s == k)
    }
    fn out(&self) -> Result<&str> {
        self.get("-o").ok_or_else(|| Error::Io("missing -o <output>".into()))
    }
    fn pos1(&self, what: &str) -> Result<&str> {
        self.pos.first().map(|s| s.as_str()).ok_or_else(|| Error::Io(format!("missing {what}")))
    }
}

fn load_source(a: &Args) -> Result<Lexer> {
    if let Some(p) = a.get("--spec") {
        return Lexer::compile(&spec_text(p)?);
    }
    if let Some(p) = a.get("--fused") {
        return Lexer::from_bytes(&read(p)?);
    }
    if let Some(p) = a.get("--unimorph") {
        return Lexer::from_unimorph(&read_text(p)?);
    }
    if let Some(p) = a.get("--wordlist") {
        return Lexer::from_wordlist(&read_text(p)?);
    }
    Err(Error::Io("need --spec, --fused, --unimorph or --wordlist".into()))
}

/// The UniMorph table for a three-letter language code, downloaded with `curl`.
/// The tables are CC BY-SA: fetched for the user, never bundled.
fn fetch_unimorph(code: &str) -> Result<String> {
    if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_lowercase()) {
        return Err(Error::Io(format!("`{code}` is not a three-letter UniMorph language code (like rus, spa, tur)")));
    }
    let url = format!("https://raw.githubusercontent.com/unimorph/{code}/master/{code}");
    eprintln!("downloading {url} (UniMorph, CC BY-SA 3.0)");
    let out = std::process::Command::new("curl").args(["-fsSL", "--max-time", "600", &url]).output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8(o.stdout).map_err(|_| Error::Io(format!("{url}: not UTF-8"))),
        _ => Err(Error::Io(format!("could not download the UniMorph table for `{code}` (is that a UniMorph language, and is curl installed?)"))),
    }
}

/// `new <code> -o out.mcfu`: build a lexer from every source given.
fn new_language(a: &Args) -> Result<()> {
    let t = Instant::now();
    let mut b = Lexer::builder();
    if let Some(p) = a.get("--spec") {
        b = b.spec(&spec_text(p)?);
    } else if let Some(p) = a.get("--unimorph") {
        b = b.unimorph(&read_text(p)?);
    } else if let Some(code) = a.pos.first() {
        b = b.unimorph(&fetch_unimorph(code)?);
    }
    if let Some(p) = a.get("--wordlist") {
        b = b.words(&read_text(p)?);
    }
    if let Some(p) = a.get("--corpus") {
        b = b.corpus(&read_text(p)?);
    }
    let lx = b.build()?;
    lx.save(a.out()?)?;
    eprintln!("built {} entries, {} stem shapes in {:.2?}; saved to {}", lx.mt.entries.len(), lx.mt.shapes.len(), t.elapsed(), a.out()?);
    Ok(())
}

/// `generate <unimorph.tsv> [-o out.morph]`: spec to the file or stdout,
/// summary and skipped rows to stderr.
fn generate(a: &Args) -> Result<()> {
    let path = a.pos1("UniMorph file")?;
    let g = morph_fst::generate::from_unimorph(&read_text(path)?)?;
    match a.get("-o") {
        Some(o) => write(o, g.spec.as_bytes())?,
        None => print!("{}", g.spec),
    }
    eprintln!("{} lemmas, {} forms, {} paradigm classes, {} irregular forms", g.lemmas, g.forms, g.paradigms, g.irregular);
    if !g.skipped.is_empty() {
        eprintln!("skipped {} row(s):", g.skipped.len());
        for (line, why) in g.skipped.iter().take(10) {
            eprintln!("  {path}:{line}: {why}");
        }
    }
    Ok(())
}

fn run(argv: &[String]) -> Result<()> {
    let Some(cmd) = argv.first() else {
        print!("{USAGE}");
        return Ok(());
    };
    let a = Args::parse(&argv[1..])?;
    match cmd.as_str() {
        "help" | "--help" | "-h" => print!("{USAGE}"),
        "new" => new_language(&a)?,
        "build" => {
            let t = Instant::now();
            let src = spec_text(a.pos1("spec")?)?;
            let lx = match a.get("--corpus") {
                Some(c) => {
                    let (lx, ind) = Lexer::compile_with_corpus(&src, &read_text(c)?)?;
                    eprintln!("learned {} exception(s) from {} tokens", ind.exceptions().count(), ind.tokens);
                    lx
                }
                None => Lexer::compile(&src)?,
            };
            let bytes = lx.to_bytes();
            write(a.out()?, &bytes)?;
            eprintln!(
                "fused lexer: {} entries, {} trie nodes, {} pieces, {} bytes, built in {:.2?}",
                lx.mt.entries.len(),
                lx.trie.n_nodes(),
                lx.trie.pieces.len(),
                bytes.len(),
                t.elapsed()
            );
        }
        "induce" => {
            let src = spec_text(a.pos1("spec")?)?;
            let cpath = a.pos.get(1).ok_or_else(|| Error::Io("missing corpus".into()))?;
            let ind = morph_fst::induce::induce(&spec::parse_spec(&src)?, &read_text(cpath)?)?;
            if ind.decisions.is_empty() {
                eprintln!("nothing to learn: the spec has no INDUCE line, or no root of an INDUCE lexicon occurs in the corpus");
                return Ok(());
            }
            println!("{} tokens, {} types", ind.tokens, ind.types);
            println!("{:<10} {:<12} {:<8} {:<18} {:>8} {:>8}  other", "lexicon", "root", "feature", "decision", "support", "surface");
            for d in &ind.decisions {
                let dec = d.flag.as_deref().map_or("(regular)".to_string(), |f| format!("@{f}"));
                let others: Vec<String> = d.alternatives.iter().map(|(f, n)| format!("@{f}:{n}")).collect();
                println!(
                    "{:<10} {:<12} {:<8} {:<18} {:>8} {:>8}  {}{}",
                    d.lexicon,
                    compose_hangul(&d.surface),
                    d.feature,
                    dec,
                    d.support,
                    d.surface_support,
                    others.join(" "),
                    if d.ambiguous { "  AMBIGUOUS" } else { "" }
                );
            }
            if let Some(o) = a.get("-o") {
                write(o, ind.annotate(&src).as_bytes())?;
                eprintln!("wrote {o} ({} learned flag(s))", ind.exceptions().count());
            }
        }
        "generate" => generate(&a)?,
        "dump-expanded" => {
            let g = spec::load(&spec_text(a.pos1("spec")?)?)?;
            for e in &g.entries {
                let f = &g.filters[e.filter as usize];
                let filt = match f.kind {
                    spec::FilterKind::None => String::new(),
                    k => format!(
                        "  {:?}{{{}}}",
                        k,
                        f.ids.iter().map(|&i| g.lex_surfaces[i as usize].as_str()).collect::<Vec<_>>().join(",")
                    ),
                };
                let rule = e.rule.as_deref().map(|r| format!("  [{r}]")).unwrap_or_default();
                let guard = if e.guard == 0 {
                    String::new()
                } else {
                    let (t, rt) = (&g.tracker, &g.rtracker);
                    let pairs: Vec<(u32, u32)> =
                        (0..t.n_regs).flat_map(|r| (0..rt.n_regs).map(move |q| (r, q))).filter(|&(r, q)| g.guard_has(e.guard, r, q)).collect();
                    let project = |tr: &spec::Tracker, f: usize, pick: &dyn Fn(&(u32, u32)) -> u32| {
                        let mut vs: Vec<&str> = pairs.iter().map(|p| tr.feat_values[f][tr.value(pick(p), f)].as_str()).collect();
                        vs.sort_unstable();
                        vs.dedup();
                        (vs.len() < tr.feat_values[f].len()).then(|| format!("{}={}", tr.feat_names[f], vs.join("|")))
                    };
                    let mut parts: Vec<String> = (0..t.feat_names.len()).filter_map(|f| project(t, f, &|p| p.0)).collect();
                    parts.extend((0..rt.feat_names.len()).filter_map(|f| project(rt, f, &|p| p.1).map(|s| format!("→{s}"))));
                    format!("  when {}", parts.join(" "))
                };
                println!(
                    "{:<12} {:<14} {:<10} -> {:<12}{}{}{}",
                    g.classes[e.lexicon as usize].name,
                    compose_hangul(&e.surface),
                    e.tag,
                    g.classes[e.cont as usize].name,
                    filt,
                    guard,
                    rule
                );
            }
            eprintln!("{} entries, {} filters", g.entries.len(), g.filters.len());
        }
        "analyze" => {
            let lx = load_source(&a)?;
            for w in &a.pos {
                let w = &morph_fst::normalize::normalize(w, &lx.mt.norm);
                match lx.all_analyses(w, 64) {
                    Some(WordOutput::Analyzed { all, .. }) => {
                        for an in all {
                            println!("{}\t{}\t[{}]", compose_hangul(w), analysis_string(&an), segmentation(&an));
                        }
                    }
                    _ => println!("{}\t+?", compose_hangul(w)),
                }
            }
        }
        "tokenize" => {
            let mut lx = load_source(&a)?;
            let mode = match a.get("--mode").unwrap_or("strict") {
                "strict" => Mode::Strict,
                "hybrid" => Mode::Hybrid,
                m => return Err(Error::Io(format!("unknown mode {m}"))),
            };
            let text = if a.pos.is_empty() {
                let mut s = String::new();
                std::io::stdin().read_to_string(&mut s)?;
                s
            } else {
                a.pos.join(" ")
            };
            if a.has("--keep-case") {
                lx.mt.norm.case = spec::CaseMode::None;
            }
            let results = lx.lex_text_par(&text, mode, 0);
            if a.has("--tokens") {
                let toks: Vec<String> = results.iter().flat_map(|(_, o)| o.tokens()).collect();
                println!("{}", toks.join(" "));
            } else {
                for (w, o) in results {
                    let w = compose_hangul(&w);
                    println!("{w:<18} {}  {}", o.source(), o.render());
                }
            }
        }
        "bench" => {
            let n = a.get("--words").map_or(Ok(100_000), |s| s.parse()).map_err(|_| Error::Io("bad --words".into()))?;
            let reps = a.get("--reps").map_or(Ok(11), |s| s.parse()).map_err(|_| Error::Io("bad --reps".into()))?;
            bench(&spec_text(a.pos1("spec")?)?, n, reps)?
        }
        other => return Err(Error::Io(format!("unknown command `{other}` (try `help`)"))),
    }
    Ok(())
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn bench(spec_text: &str, n_words: usize, reps: usize) -> Result<()> {
    let reps = reps.max(1);
    // ---- build time ----------------------------------------------------
    let mut times = Vec::new();
    for _ in 0..21 {
        let t = Instant::now();
        let f = Lexer::compile(spec_text)?;
        let bf = f.to_bytes();
        times.push(t.elapsed().as_secs_f64() * 1e3);
        std::hint::black_box(bf);
    }
    let g = spec::load(spec_text)?;
    let fused = Lexer::from_grammar(&g);
    println!("build (median of 21, incl. serialization)   [{} entries]", g.entries.len());
    println!("  {:7.2} ms   {} bytes", median(times), fused.to_bytes().len());

    // ---- corpus: 50% grammatical, 25% typo'd, 25% random ----------------
    let ast = spec::parse_spec(spec_text)?;
    let gen = oracle::generate(&ast, 4, 300_000)?;
    let good: Vec<&String> = gen.surfaces.keys().collect();
    // alphabet: the language's own letters (single-char word-initial pieces)
    let alpha: Vec<char> = derive_vocab(&g)
        .iter()
        .filter(|p| !p.cont && p.text.chars().count() == 1)
        .filter_map(|p| p.text.chars().next())
        .filter(|c| c.is_alphabetic())
        .collect();
    let mut rng = Rng::new(7);
    let mut words = Vec::with_capacity(n_words);
    for i in 0..n_words {
        let w = match i % 4 {
            0 | 1 => rng.pick(&good).to_string(),
            2 => {
                let mut cs: Vec<char> = rng.pick(&good).chars().collect();
                let k = rng.below(cs.len());
                match rng.below(3) {
                    0 => cs.insert(k, *rng.pick(&alpha)),
                    1 if cs.len() > 1 => {
                        cs.remove(k);
                    }
                    _ => cs[k] = *rng.pick(&alpha),
                }
                cs.into_iter().collect()
            }
            _ => (0..3 + rng.below(10)).map(|_| *rng.pick(&alpha)).collect(),
        };
        words.push(w);
    }
    let bytes: usize = words.iter().map(|w| w.len()).sum();
    println!("throughput on {n_words} words ({:.1} KB; 50% grammatical, 25% typos, 25% random), median of {reps}", bytes as f64 / 1e3);
    let run = |name: &str, f: &dyn Fn(&str) -> WordOutput| {
        let mut times = Vec::new();
        let mut fst_hits = 0usize;
        for rep in 0..reps {
            let t = Instant::now();
            for w in &words {
                let o = f(w);
                if rep == 0 && matches!(o, WordOutput::Analyzed { .. }) {
                    fst_hits += 1;
                }
                std::hint::black_box(o);
            }
            times.push(t.elapsed().as_secs_f64());
        }
        let s = median(times);
        println!(
            "  {name:<26} {:>9.0} words/s  {:>6.2} MB/s   FST-accepted {:.1}%",
            n_words as f64 / s,
            bytes as f64 / s / 1e6,
            100.0 * fst_hits as f64 / n_words as f64
        );
    };
    run("strict", &|w| fused.lex(w, Mode::Strict));
    run("hybrid", &|w| fused.lex(w, Mode::Hybrid));
    run("LinMaxMatch only", &|w| WordOutput::Fallback(fused.trie.tokenize(w)));
    Ok(())
}
