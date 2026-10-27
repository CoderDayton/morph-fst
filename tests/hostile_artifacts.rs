//! Crafted artifacts: corrupt the payload, then re-wrap it with a valid
//! header and checksum, so loading gets past the checksum and every table
//! check is exercised. Loading must fail with `Err`, or produce a lexer
//! that lexes without panicking.
//!
//! Own test binary: it installs a global panic hook to collect locations.

use morph_fst::codec;
use morph_fst::{Lexer, Mode};
use morph_fst::rng::Rng;
use morph_fst::languages;
use std::collections::BTreeMap;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Mutex;

type TestResult = Result<(), Box<dyn std::error::Error>>;

static PANICS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Underlying-mode harmony and both pin kinds, which the bundled
/// languages do not all cover.
const UNDERLYING: &str = r#"
OPTION harmony underlying
CLASS FRONT = e i ï
CLASS BACK = a o u
FEATURE h
  [FRONT] -> f
  [BACK] -> b
RFEATURE r
  [FRONT] -> f
  [BACK] -> b
ARCHI A
  h=f -> e
  * -> a
ARCHI P
  r=b -> o
  * -> i
LEXICON Root
  0 Pfx ;
LEXICON Pfx
  m{P}   P   Stems ;
  0 Stems ;
LEXICON Stems @open=S
  kït    S   Sfx ;
  kot    S   Sfx ;
  pil    S   Sfx @tier=pul ;
LEXICON Sfx
  l{A}r  PL  # ;
  0 # ;
RULES
  absn: ï -> a / _ [*]* +{l{A}r} ;
"#;

const PINS: &str = r#"
CLASS PLUS = i e
CLASS MINUS = a o
RFEATURE h
  [PLUS] -> p
  [MINUS] -> m
FEATURE lf
  [PLUS] -> p
  [MINUS] -> m
ARCHI A
  h=m -> a
  * -> e
ARCHI O
  lf=m -> o
  * -> i
LEXICON Root
  0 P1 ;
LEXICON P1
  b{A}  P1  Stems ;
  0 Stems ;
LEXICON Stems @open=S
  tik   S  Sfx ;
  tok   S  Sfx ;
  pil   S  Sfx @rset:h=m @set:lf=m ;
LEXICON Sfx
  n{O}  N  # ;
  0 # ;
"#;

const WORDS: &[&str] = &[
    "", "a", "evlerimizden", "kitapta", "walked", "unhappiness", "canciones", "commençons", "학생이",
    "ịga", "riri", "katler", "mikït", "bapilno", "tikni", "googlelar", "zzzq", "😀x", "'", "#ab",
];

/// Overwrite 1–3 spots of the payload, favouring values that make a
/// length or index field point out of range.
fn mutate(payload: &mut [u8], rng: &mut Rng) {
    const NASTY: [u32; 8] = [0, 1, 2, 3, 0xFF, 0xFFFF, 0x7FFF_FFFF, u32::MAX];
    for _ in 0..1 + rng.below(3) {
        let k = rng.below(payload.len());
        if rng.chance(1, 2) {
            let v = if rng.chance(1, 2) { *rng.pick(&NASTY) } else { rng.below(1 << 16) as u32 };
            for (i, b) in v.to_le_bytes().iter().enumerate() {
                if let Some(slot) = payload.get_mut(k + i) {
                    *slot = *b;
                }
            }
        } else if let Some(slot) = payload.get_mut(k) {
            *slot ^= 1 << rng.below(8);
        }
    }
}

fn exercise(lx: &Lexer) {
    for w in WORDS {
        let _ = lx.lex(w, Mode::Strict);
        let _ = lx.lex(w, Mode::Hybrid);
        let _ = lx.analyze(w, 64);
    }
    let _ = Lexer::from_bytes(&lx.to_bytes());
}

#[test]
fn crafted_artifacts_never_panic() -> TestResult {
    panic::set_hook(Box::new(|info| {
        let loc = info.location().map_or_else(|| "?".to_string(), |l| format!("{}:{}", l.file(), l.line()));
        let msg = info.payload().downcast_ref::<String>().cloned().or_else(|| info.payload().downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
        if let Ok(mut p) = PANICS.lock() {
            p.push(format!("{loc}  {msg}"));
        }
    }));
    let iters: usize = std::env::var("HOSTILE_N").ok().and_then(|s| s.parse().ok()).unwrap_or(1500);
    let specs = languages::ALL.iter().copied().chain([("underlying", UNDERLYING), ("pins", PINS)]);
    let mut rng = Rng::new(41);
    let (mut loaded, mut rejected) = (0usize, 0usize);
    for (name, text) in specs {
        let data = Lexer::compile(text)?.to_bytes();
        let magic: [u8; 4] = data.get(..4).and_then(|m| m.try_into().ok()).ok_or("short artifact")?;
        let payload = data.get(24..).ok_or("short artifact")?;
        for i in 0..iters {
            let mut p = payload.to_vec();
            mutate(&mut p, &mut rng);
            let bytes = codec::wrap(&magic, &p);
            let r = panic::catch_unwind(AssertUnwindSafe(|| match Lexer::from_bytes(&bytes) {
                Ok(lx) => {
                    exercise(&lx);
                    true
                }
                Err(_) => false,
            }));
            match r {
                Ok(true) => loaded += 1,
                Ok(false) => rejected += 1,
                Err(_) => {
                    if let Ok(mut p) = PANICS.lock() {
                        if let Some(last) = p.last_mut() {
                            *last = format!("{last}   [{name} #{i}]");
                        }
                    }
                }
            }
        }
    }
    let _ = panic::take_hook();
    let panics = PANICS.lock().map(|p| p.clone()).unwrap_or_default();
    let mut by_loc: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for p in &panics {
        let loc = p.split("  ").next().unwrap_or("").to_string();
        let e = by_loc.entry(loc).or_insert((0, p.clone()));
        e.0 += 1;
    }
    eprintln!("loaded {loaded}, rejected {rejected}, panicked {}", panics.len());
    for (n, example) in by_loc.values() {
        eprintln!("{n:>5}x  {example}");
    }
    assert!(panics.is_empty(), "{} crafted artifacts panicked at {} distinct sites", panics.len(), by_loc.len());
    Ok(())
}
