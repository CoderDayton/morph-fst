# morph-fst

A rule-based **morphological analyzer** fused with a **deterministic subword fallback**, in Rust, with no dependencies.

You write a language's morphology once, as a plain-text `.morph` spec. The crate compiles it into one artifact that, for every word:

1. **parses it** into known roots and affixes if the grammar accepts it:
   - `unhappiness → un<PFX:NEG>+happy<ADJ>+ness<NMLZ>`
   - `evlerimizden → ev<N>+lAr<PL>+IxmIz<P1PL>+DAn<ABL>` (Turkish vowel harmony)
   - `ịga → I<INF>+ga<V:go>` vs `iri → I<INF>+ri<V:eat>` (Igbo prefixes agree with the root after them)
2. **otherwise splits it deterministically** into WordPiece subwords (LinMaxMatch, linear time). The vocabulary is derived from the grammar. It never fails and never emits `[UNK]`; unknown characters become bytes.
3. **optionally (hybrid mode)** finds known affixes around one unknown stem: `bloglarımda → ⟨blog⟩<?N> + lAr<PL> + Ixm<P1SG> + DA<LOC>`.

It ships with demo grammars for **English, Turkish, Spanish, French, Korean and Igbo**.

## Install

```toml
[dependencies]
morph-fst = "0.1"
```

The minimum supported Rust version is 1.82. The default build has **zero dependencies**.

## Use

```rust
use morph_fst::{Lexer, Mode, WordOutput};

let lx = Lexer::bundled("turkish")?;               // or Lexer::compile(&my_spec_text)?

for (word, out) in lx.lex_text("Kitaplarımızdan bloglarımda", Mode::Hybrid) {
    match out {
        WordOutput::Analyzed { best, .. } => { /* best: Vec<Morph { surface, lemma, tag, start, end }> */ }
        WordOutput::Hybrid { segments, .. } => { /* known affixes + one Segment::Unknown stem */ }
        WordOutput::Fallback(pieces) => { /* WordPiece split; concatenation == word */ }
    }
}

// flat tokens for a downstream model
let toks = lx.lex("evlerde", Mode::Strict).tokens();   // ["ev<N>", "lAr<PL>", "DA<LOC>"]

// compile once, ship one file; loading validates every table
let bytes = lx.to_bytes();
let lx = Lexer::from_bytes(&bytes)?;
```

- `lex_text` normalizes the text the way the language's spec says (Turkish `I→ı`, tone stripping, Hangul → jamo, …) and splits it into words.
- `lex` takes a single word that is already normalized.
- `analyze(word, cap)` returns every analysis, not just the best one.

Runnable examples:

```sh
cargo run --example quickstart
cargo run --example learn_exceptions
```

### Command line

```sh
cargo install morph-fst --features cli

morph-fst tokenize --spec turkish "Kitaplarımızdan geliyorum, saatler evlerda"
morph-fst tokenize --spec english --mode hybrid "unhappyness blorfing"
morph-fst analyze  --spec spanish empiece canciones
morph-fst build    my.morph -o my.mcfu  [--corpus text.txt]
morph-fst tokenize --fused my.mcfu < input.txt
morph-fst induce   my.morph text.txt -o my.learned.morph
morph-fst dump-expanded turkish        # every concrete allomorph + when it applies
morph-fst bench turkish
```

`--spec` takes a file path or a bundled language name. Example output:

```
kitaplarımızdan  FST  kitap<N>+lAr<PL>+IxmIz<P1PL>+DAn<ABL>   [kitap|lar|ımız|dan]
geliyorum        FST  gel<V>+Ixyor<PROG>+YIm<1SG>              [gel|iyor|um]
saatler          FST  saat<N>+lAr<PL>                          [saat|ler]
evlerda          SUB  ev ##ler ##da                            (harmony violation → fallback)
empiece          FST  empezar<V>+e<SBJV.1/3SG>                 [empiec|e]
l'homme          FST  le<DET>+homme<N>                         [l'|homme]
갔어요            FST  가<V>+EOᆻ<PST>+어요<POL>                  [가|ᆻ|어요]
ịkpụ             HYB  I<INF> + ⟨kpụ⟩<?V>{k ##pụ}                (unknown root still selects ị-)
```

### Cargo features

| Feature | What it adds |
|---|---|
| *(default)* | `Lexer`, the spec compiler, exception learning, bundled languages. No dependencies. |
| `cli` | The `morph-fst` binary. |

## Why one lexer

A morphological analyzer with a subword fallback is usually two things: an FST and a separately compiled WordPiece model. That means two compile loops, two artifacts, and two sources that can drift apart. Here the subword vocabulary is derived from the grammar. Morphemes and pieces go into **one trie in one loop**, the failure links come from **one BFS**, and the morphological walkers and LinMaxMatch run in **one scan**. The result is one file, and nothing that can drift.

Underlying-form harmony and exception learning are **not** separate modes or add-ons. They are part of the same compiler, artifact format and scan. Each language's spec declares whether it uses them (`OPTION harmony underlying`, `INDUCE …`), because they are facts about that language. When a spec doesn't use a feature, its code never runs: for English, Turkish, Spanish, French and Korean, the compiled grammars, tries, lattices and outputs are byte-identical with or without these features (see [Performance](#performance)).

## The spec language

```
OPTION case turkish|lower|none        OPTION hangul_jamo true      OPTION apostrophe_in_words true
OPTION strip_tones true               ! drop tone marks, keep dot below/above (Igbo ị́ → ị)
OPTION harmony surface|underlying     ! what harmony reads (default: surface)
OPTION induce_min_count 2             ! evidence needed to learn an exception
CLASS VOWELISH = a e ı i o ö u ü {A} {I}      ! chars and archiphonemes

FEATURE vow                           ! left context: an automaton over the text before
  a ı -> bu                           ! atoms: chars, [CLASS], [*]  ;  first match wins
  e i -> fu
ARCHI A                               ! cond -> output ; `*` = otherwise ; `0` = empty
  vow=bu|br -> a
  * -> e

RFEATURE atr                          ! right context: same syntax, run from the END
  [PLUS] -> plus
  [MINUS] -> minus
ARCHI I                               ! a prefix reading right context
  atr=minus -> ị
  * -> i

INDUCE vow IN Nouns Adjs              ! roots here may be exceptions: learn which

LEXICON Plural
  l{A}r   PL  Poss ;                  ! surface[:lemma] TAG Cont [@flags] ;
  0 Poss ;                            ! epsilon
LEXICON Nouns @open=N                 ! hybrid mode may posit an unknown N stem
  kitap   N   NounCont @soft ;        ! flags gate rules
  saat    N   NounCont @set:vow=fu ;  ! hand-marked exception (or let INDUCE learn it)
  l':le   DET Nouns @next=VH ;        ! next morpheme must start with class VH
  pil     S   Sfx @rset:h=m ;         ! right-context exception
  pil     S   Sfx @tier=pul ;         ! underlying mode: harmonizes as if it were `pul`

RULES                                 ! name[?]: TARGET -> REPL / [LEFT] _ [TAIL] +RIGHT [@flag] ;
  soft_p:  p -> b    / _ +[VOWELISH] @soft ;
  e_ie:    e -> ie   / _ [C]+ +{o,as,a,an,es,e,en} @ie ;   ! TAIL anchored at the morpheme end
  wa_opt?: ᅩ -> ᅪ    / _ +[EO] @wa_opt ;                    ! optional rule (both forms valid)
```

`dump-expanded` shows exactly what the compiler produced:

```
Case  de   LOC -> #   when vow=fr|fu|none coda=none|v|vd
Case  te   LOC -> #   when vow=fr|fu|none coda=vl
Nouns kitab N  -> NounCont  Allow{...vowel-initial suffixes...}  [soft_p]
```

### Long-distance context, both directions

- **`FEATURE`: left context.**
  - It is a small deterministic automaton over the characters before a position: "backness/rounding of the last vowel", "is the last consonant voiceless".
  - Archiphonemes (`l{A}r`, `{D}{A}n`) are realized **ahead of time** for every register value and grouped into concrete allomorphs (`lar`/`ler`), each guarded by a bitset.
  - At runtime the register costs one table lookup per character and one bit test per arc. It adds no lattice states.
- **`RFEATURE`: right context.**
  - It is the same automaton, run backward from the end of the word, for prefixes that agree with what follows (Igbo i-/ị-) and for dominant harmony.
  - It costs one extra O(n) pass, only for languages that declare it.
  - Archiphonemes can read left, right or both. Guards become bitsets over (left, right) register pairs.

### Opaque harmony: `OPTION harmony underlying`

By default harmony is **surface-true**: registers read the final text. In many languages that is wrong:
- a rule changes a vowel, but the suffixes still harmonize with the *original* vowel;
- a root behaves as if it had different vowels.

With `OPTION harmony underlying`:

- Each morpheme contributes its **pre-rule form** to the harmony tier. Rules still decide the surface, but harmony no longer sees their effect.
- `@tier=xyz` gives a morpheme an abstract tier form. For example `pil @tier=pul` is a root spelled with a front vowel that harmonizes as back.
- The registers move into the lattice state. They are carried morpheme by morpheme instead of being read off the surface. Right context uses guess-and-verify with an inverse table, so analyses never duplicate.
- An unknown stem in hybrid mode contributes its own letters to the tier.

Example (`tests/exceptions.rs`): `ï` is front, but a rule turns it into `a` before the plural. The plural still harmonizes with `ï`, so `kït+lAr → katler` (not `*katlar`), and the prefix agrees with the underlying root: `mikatler`.

### Lexical exceptions, learned instead of marked

Turkish *saat* "hour" has back vowels but takes front suffixes (*saatler*, not *saatlar*). Such roots can be marked by hand (`@set:vow=fu`), or the spec can say **where** exceptions may occur and learn **which** roots are exceptions from plain text:

```
INDUCE vow IN Nouns Adjs
```

```rust
let (lexer, learned) = Lexer::compile_with_corpus(&spec_text, &raw_text)?;
for d in learned.exceptions() {
    println!("{} @{} (support {} vs {})", d.surface, d.flag.as_deref().unwrap_or(""), d.support, d.surface_support);
}
std::fs::write("learned.morph", learned.annotate(&spec_text))?;   // review + check in
```

How it works (the `induce` module):
1. It builds a hypothesis grammar in which each root of an INDUCE lexicon exists once per hypothesis: regular, or pinned to each value of each inducible feature.
2. It analyzes every word type of the corpus.
3. A hypothesis gets support from a word only when that word discriminates between the hypotheses.
4. A root becomes an exception only if one hypothesis beats the regular reading and reaches `induce_min_count` tokens.
5. Ties are reported as ambiguous, and roots that are already hand-marked are left alone.

The output is an ordinary spec, so the lexer is exactly what hand marking would produce. On the 90-token sample in `data/turkish_corpus.txt`, it recovers exactly the four hand-marked roots of the bundled Turkish spec (*saat, harf, hayal, kalp*), and the learned lexer gives the same output as the hand-marked one (tested on 20k words).

INDUCE also applies **at runtime**: in hybrid mode an unknown stem may carry an exception when its suffixes require one. For example `googlelar → ⟨google⟩<?N vow=bu> + lAr<PL>` reports that the stem behaves as back-vowelled. Cost is lexicographic (unknown characters, exceptions, morphemes), so for the same unknown stem a regular reading always beats one with an exception.

The bundled Turkish spec keeps its hand marks and has no INDUCE line, so its hybrid mode stays as fast as before. Add `INDUCE` to your own copy to get runtime inference.

## Languages

| Spec | Shows | Examples (all in tests) |
|---|---|---|
| `english` | spelling rules, derivation | unhappiness, stopped, happily, unbelievably |
| `turkish` | 2- and 4-way vowel harmony across suffix chains; d/t, c/ç; buffer y/s/n; softening; vowel drop; doubling; exceptional roots | evlerimizden, kitabı / kitapta, saatler, kalbi, geliyorum, gelmiyorum, geleceğim, burnu, hakkı |
| `spanish` | {S} plural; stem changes feeding c/qu, g/gu, z/c; accent loss; in- assimilation | canciones, lápices, piensan, empiece, almuerce, busqué, imposible, irreal |
| `french` | ç/ge before a/o; è/ll/tt stems; -al→-aux; feminine x/f/er; elision | mangeons, commençons, achète, appellerai, chevaux, heureusement, l'homme |
| `korean` | Hangul→jamo; 이/가, 을/를, 은/는, (으)로; 아/어 harmony and contraction; ㅡ ㅂ ㄷ 르 하 irregulars | 학생이, 서울로, 먹어요, 갔어요, 해요, 추워요, 몰라요, 들으면 |
| `igbo` (simplified, tones stripped) | ATR harmony in **both** directions | iri / ịga, inye / ịṅụ, eri / aga, riri / gara / hụrụ, erighị / agaghị |

These are compact demo lexicons (a few hundred morphemes each) that show the spec language. Extend them, or write your own.

**Normalization** is per language and applied by `lex_text` / `Lexer::normalize`:
- Latin combining marks are composed.
- Case folding is per language (Turkish `I→ı`, `İ→i`).
- `’` becomes `'` (and stays inside French words).
- `strip_tones` removes tone marks in any order while keeping dot below/above.
- Hangul is decomposed arithmetically into jamo, so a morpheme can end mid-syllable (가 + ᆻ = 갔). Output is recomposed for display.

## Performance

Numbers are instructions per word under valgrind/callgrind, on the same pinned corpora. Wall-clock time in a shared container varies ±15%, so instruction counts are the reference.

| | Strict | Hybrid |
|---|---|---|
| English | 14.7k | 40.4k |
| Turkish | 20.4k | 44.5k |

These were measured after adding underlying harmony and exception learning, and compared with the version before them:
- **Strict mode** is unchanged or slightly faster: English −0.3%, Turkish −1.1% to −1.4%.
- **Hybrid mode:** English −0.4%, Turkish +0.4%.
- **Builds:** +0.5%.

For every bundled language except Igbo (which is covered by its golden tests), these are byte-identical to the previous version: expanded grammar, derived vocab, trie, every per-word lattice, and the outputs on the pinned corpora.

## Verification

Run the full suite with `cargo test --release`.

- **Independent oracle** (`src/oracle.rs`).
  - It is a separate interpreter of the spec AST, with its own register simulation, archiphoneme resolution (branch-and-verify for right context, tier-based for underlying mode) and rule matcher.
  - For every language, the lexer must accept exactly the surfaces the oracle generates (up to 4 morphemes) with exactly the same glosses. It must also reject every rule-less form and every wrong archiphoneme choice: about 40k surfaces and 150k forbidden forms.
- **Hybrid ≡ strict** on every word strict mode accepts: 25k random words per language, 5k–8k per toy spec.
- **Mutation checks.** Each of these makes the tests fail:
  - disabling the guard check;
  - disabling left pins or right pins;
  - ignoring the right register;
  - freezing the underlying-mode register.
- **LinMaxMatch** is checked against a naive greedy reference on 1,500 random vocabularies.
- **Robustness.**
  - The fallback is lossless on random Unicode.
  - 1 MB words are handled.
  - Corrupted artifacts give `Err`, never a panic. This includes crafted ones re-signed with a valid checksum (12k per run; set `HOSTILE_N` for more). Any that load must then lex without panicking.
  - Builds are deterministic.

## Limitations

- **Ordering inside a morpheme.** Left-reading archiphonemes must come before right-reading ones, with at most one both-ways reader between them. In surface mode, a rule may not rewrite text that an archiphoneme in the same morpheme already read. Both are compile errors.
- **What rules can see.** Rules see the next morpheme's lexical form or first symbol; long-distance conditions go through FEATUREs.
- **Exception learning.**
  - It needs the root to occur with discriminating suffixes.
  - It learns one value per (root, feature).
  - In hybrid mode it explains one unknown stem per word.
- **Igbo is simplified.** Tones are ignored, and the negative is `-ghị` throughout.
- **Demo-sized lexicons.** Some irregulars are omitted.
- **Korean segments are in jamo.** A boundary inside a syllable shows as partial jamo.
- **Punctuation** detection covers ASCII and the main Unicode punctuation blocks (`std` has no Unicode category tables).

## Layout

```
data/                 bundled .morph specs, Turkish sample corpus
src/spec/             parser + compiler: registers, archiphonemes, rules, guards, pins, pruning
src/morphotactics.rs  lattice states, register tables, guards, pins, underlying-mode registers
src/fused/            the lexer: builder (one loop), scan, hybrid, artifact
src/induce.rs         exception learning
src/subword/          trie + failure links, LinMaxMatch, naive reference
src/oracle.rs         independent reference interpreter (for tests)
```

## References

- Song et al., *Fast WordPiece Tokenization*, EMNLP 2021 (LinMaxMatch). https://aclanthology.org/2021.emnlp-main.160
- Beesley and Karttunen, *Finite State Morphology*, 2003 (lexc continuation classes, two-level rules).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
