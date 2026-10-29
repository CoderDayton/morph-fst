# morph-fst

A rule-based **morphological analyzer** fused with a **deterministic subword fallback**, in Rust.

You write a language's morphology once, as a plain-text `.morph` spec. The crate compiles it into one artifact that, for every word:

1. **parses it** into known roots and affixes if the grammar accepts it:
   - `unhappiness → un<PFX:NEG>+happy<ADJ>+ness<NMLZ>`
   - `evlerimizden → ev<N>+lAr<PL>+IxmIz<P1PL>+DAn<ABL>` (Turkish vowel harmony)
   - `ịga → I<INF>+ga<V:go>` vs `iri → I<INF>+ri<V:eat>` (Igbo prefixes agree with the root after them)
2. **otherwise splits it deterministically** into WordPiece subwords (LinMaxMatch, linear time). The vocabulary is derived from the grammar. It never fails and never emits `[UNK]`; unknown characters become bytes.
3. **for a word it has never seen (hybrid mode)** reads the unknown stem against the stems it knows and returns every reading the grammar supports, best first:
   - `blorties → ⟨blortie→blorty⟩<?N> + s<PL>` (the stem takes `y→ie` from *city*, *party*)
   - `bloglarımda → ⟨blog⟩<?N> + lAr<PL> + Ixm<P1SG> + DA<LOC>`

It ships with a **full English grammar** (38k stems, generated from the permissively licensed [ESDB](https://github.com/en-wl/wordlist) word database), demo grammars for **Turkish, Spanish, French, Korean and Igbo**, and drafts grammars for other languages from [UniMorph](https://unimorph.github.io) tables.

## What it does, and the two modes

A plain lexical analyzer is a dictionary: it knows `walked = walk + PAST` because someone listed it, and it knows nothing about a word it has not seen. morph-fst replaces the list with a **grammar**: stems, the endings each stem class takes, and the spelling rules between them. Every known word is a path through that grammar.

| Mode | Known word | Unknown word |
|---|---|---|
| `Strict` | full analysis (`stopped → stop<V>+ed<PST>`) | WordPiece subwords (`bl ##orf ##ing`); never fails |
| `Hybrid` | identical to strict | ranked analyses with a guessed stem, its lemma and class (`blopped → ⟨blopp→blop⟩<?V> + ed<PST>`); subwords only if nothing fits |

Use `Strict` when you want only analyses the grammar fully licenses; use `Hybrid` for open text, where new words are the norm.

By default an unknown word is **guessed** (`Mode::Hybrid`, `Kind::Guessed`). `Mode::Strict` leaves it **unsplit** into subword pieces (`Kind::Unsplit`) instead.

## Install

```toml
[dependencies]
morph-fst = "0.1"
```

The minimum supported Rust version is 1.82. Dependencies are kept few and lightweight, and only where they beat writing the code here.

## Use

Three steps: get a `Lexer`, `analyze` words, read the `Word`.

```rust
use morph_fst::{Kind, Lexer};

let lx = Lexer::bundled("turkish")?;                 // 1. get a lexer

for word in lx.analyze_text("Kitaplarımızdan bloglarımda") {   // 2. analyze
    println!("{:?}", word.kind());                   // Known | Guessed | Unsplit
    for part in word.parts() {                       // 3. read it
        println!("{} {} {}", part.text(), part.lemma(), part.tag());   // lAr  lar  PL
    }
}

lx.analyze("evlerde").tokens();                      // ["ev<N>", "lAr<PL>", "DA<LOC>"]

lx.save("turkish.mcfu")?;                            // compile once, reuse the file
let lx = Lexer::load("turkish.mcfu")?;
```

| You want | Call |
|---|---|
| A ready-made language | `Lexer::bundled("english")` |
| Your own language | `Lexer::builder()`, see [Generating a language](#generating-a-language) |
| One word | `lx.analyze(word)` |
| Running text | `lx.analyze_text(text)`: normalized (Turkish `I→ı`, tone stripping, Hangul → jamo, …), split into words, and spread over all cores when long (each distinct word is lexed once) |
| Skip the guessing | `Lexer::bundled("english")?.mode(Mode::Strict)` |
| Everything about a word | `word.detail()`: byte spans, every reading, subword pieces |
| Save and reload | `lx.save(path)`, `Lexer::load(path)` |

A `Word` has `text()`, `kind()`, `parts()`, `alternatives()` (other readings, best first), `tokens()` and `Display`. A `Part` has `text()`, `lemma()`, `tag()` and `is_known()`. On 18k unseen English words, guessing takes 881 ms on one thread and 165 ms on 32 cores.

Runnable examples:

```sh
cargo run --example quickstart
cargo run --example learn_exceptions
```

### Generating a language

Give the builder whatever data exists, in any combination:

```rust
let lx = Lexer::builder()
    .unimorph(&tables)    // lemma<TAB>form<TAB>FEATURES: features and lemmas (richest)
    .words(&word_list)    // plain words: covers what the tables miss
    .corpus(&raw_text)    // raw text: learns exceptions (a spec's INDUCE lines)
    .build()?;            // or .spec(&text) instead of .unimorph(..) for a hand-written grammar
```

| You have | You get |
|---|---|
| UniMorph tables | stems, endings with features, lemmas, spelling-change variants |
| A word list only | stems and endings (no features), found by [paradigm discovery](#discovering-a-grammar-from-a-word-list) |
| Tables and a word list | the tables' grammar, plus paradigms discovered from the words the tables reject |
| A spec | exactly your grammar; a word list or corpus adds to it |

From the command line one command does it, including the download:

```sh
morph-fst new rus -o russian.mcfu                          # fetches UniMorph's Russian table
morph-fst new rus -o russian.mcfu --wordlist words.txt     # plus your words
morph-fst tokenize --fused russian.mcfu "я люблю читать книги"
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
morph-fst generate unimorph.tsv -o draft.morph   # draft a spec from UniMorph tables
morph-fst tokenize --unimorph rus.tsv --mode hybrid "я люблю читать книги"   # no spec at all
morph-fst tokenize --wordlist words.txt --mode hybrid "some unseen words"      # not even tables
morph-fst dump-expanded turkish        # every concrete allomorph + when it applies
morph-fst bench turkish
```

In hybrid output, `⟨blopp→blop⟩<?V>{b ##l ##o ##p ##p}` is an unknown stem: its surface, the lemma the spelling rules imply, the guessed class, and its subword pieces.

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
| *(default)* | `Lexer`, the spec compiler, exception learning, bundled languages. |
| `cli` | The `morph-fst` binary. |

## Why one lexer

A morphological analyzer with a subword fallback is usually two things: an FST and a separately compiled WordPiece model. That means two compile loops, two artifacts, and two sources that can drift apart. Here the subword vocabulary is derived from the grammar. Morphemes and pieces go into **one trie in one loop**, the failure links come from **one BFS**, and the morphological walkers and LinMaxMatch run in **one scan**. The result is one file, and nothing that can drift.

Underlying-form harmony and exception learning are **not** separate modes or add-ons. They are part of the same compiler, artifact format and scan. Each language's spec declares whether it uses them (`OPTION harmony underlying`, `INDUCE …`), because they are facts about that language. When a spec doesn't use a feature, its code never runs.

## The lexicon is the model

Unknown words are handled by the same grammar that analyzes known ones. There is no second model, no training run and no tuned weight. Each piece follows from one idea: **every word is a path through the grammar, and an unknown word is a path whose stem borrows its ending from known stems.**

- **Zero morphemes** (`0 SG # ;`). A paradigm cell with no ending (*cat* = cat + SG) is a real morpheme with a tag. Without it a bare word has nothing to carry its features, and 60% of English forms are bare.
- **Stem shapes.** Every ending of every known stem is a shape: `ity → ity` from *city*, `opp → op` from *stopp-ed*. A shape remembers the stem's class, its spelling-rule flags and the lemma change, so an unknown stem that ends the same way inherits all three. Shapes come from the compiled grammar, so rules apply to unknown stems exactly as to known ones.
- **One example is an anecdote.** A shape needs two known stems (`MIN_SUPPORT`). That is the only constant.
- **Irregular stems are not patterns.** A stem whose lemma has forms listed whole (*go*, *went*) lends no shapes; *zak* should not inherit *take*'s past.
- **Ranking is lexicographic, not weighted.** Readings are ordered by (characters left unexplained, lexical exceptions, −stems supporting the shape, morphemes). The longest borrowed ending wins; among equals, the pattern more stems share wins. Every reading is returned, not just the first.

On English (UniMorph, 584k forms, 382k lemmas), every tenth lemma was held out and all its forms run in hybrid mode. These are words the analyzer has never seen:

| 65,327 forms of unseen lemmas | Analyzed | Features, top reading | Lemma, top reading | Features in some reading |
|---|---|---|---|---|
| Dictionary lookup | 0% | 0% | 0% | 0% |
| Suffix guesser trained on the same table | 100% | 78.0% | 91.6% | 91.9% |
| **morph-fst hybrid** | 100% | 77.4% | 90.7% | **96.2%** |

The suffix guesser is the standard statistical baseline (longest seen ending → most frequent tag and lemma edit). morph-fst matches it on the top reading without counting anything, and it also returns the segmentation, a lemma produced by the spelling rules, and the right reading among its candidates 96% of the time. Most of the remaining top-1 misses can't be fixed without sentence context: noun vs adjective for bare words, past vs participle for `-ed`.

Where suffix chains and harmony matter, the grammar pulls ahead. On Turkish it gets 84.5% of unseen forms right against the guesser's 74.7% (see [the UniMorph table](#drafting-a-spec-from-unimorph)).

Speed on the full English grammar: strict 8 µs per word, hybrid 86 µs per unknown word (known words take the strict path).

### The bundled English

`data/english.morph` is generated, not hand-written. Two inputs:

- **The grammar**, `tools/english/grammar.morph` (about 400 lines, written by hand): prefixes, inflection and derivation classes, 21 spelling rules, and the function words.
- **The lexicon**, from [ESDB](https://github.com/en-wl/wordlist) (the English Speller Database, formerly SCOWL v2; MIT-like license). It supplies every open-class lemma of size 60 or less (ESDB's default spell-checker size, American spelling) with its part of speech and inflected forms.

`examples/build_english.rs` fits the two together:

1. Each lemma gets the class and spelling-rule flag that reproduce its ESDB inflections. The lexer itself checks every candidate: `stop` → `@double`, `panic` → `@ck`, `knife` → `@ves`.
2. Forms no class reproduces are listed whole (*went*, *mice*, *better*): 1,521 of them.
3. A verb takes `-ment`, `-ion`, `-ation` or `-able` only when ESDB has the derived word (*movement* → `move` gets `-ment`).
4. Words the grammar derives transparently from another stem are left to the grammar (*happiness*, *unhappy*, *quickly*): 7,673 of them.

The result is 23k nouns, 6.6k verbs, 8.7k adjectives and 680 adverbs. Regenerate after editing the grammar:

```sh
git clone --depth 1 https://github.com/en-wl/wordlist
cargo run --release --example build_english -- wordlist/data/scowl-pre.txt > data/english.morph
```

Scored on all 649,583 UniMorph English forms. Most of their lemmas are not in the bundled lexicon.

| Bundled English | Entries | Lookup: features | Hybrid: features | Hybrid: lemma | Features in some reading |
|---|---|---|---|---|---|
| Demo grammar | 442 | 0.5% | 45.1% | 80.4% | 85.1% |
| Hand-written word lists | 4,819 | 3.6% | 67.2% | 90.5% | 96.6% |
| **Generated from ESDB** | 57,788 | **19.0%** | **74.7%** | **92.8%** | 95.3% |

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
LEXICON Poss
  0 SG # ;                            ! zero morpheme: a tagged cell with no ending
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

INDUCE also applies **at runtime**: in hybrid mode an unknown stem may carry an exception when its suffixes require one. For example `googlelar → ⟨google⟩<?N vow=bu> + lAr<PL>` reports that the stem behaves as back-vowelled. Cost is lexicographic (unknown characters, exceptions, shape support, morphemes), so for the same unknown stem a regular reading always beats one with an exception.

The bundled Turkish spec keeps its hand marks and has no INDUCE line, so its hybrid mode stays as fast as before. Add `INDUCE` to your own copy to get runtime inference.

### Drafting a spec from UniMorph

Specs are written by hand, but [UniMorph](https://unimorph.github.io) tables (`lemma<TAB>form<TAB>FEAT;FEAT;…`, 180+ languages) give a starting point:

```sh
morph-fst generate spa.tsv -o spanish-draft.morph
```

To skip the spec file, use `Lexer::builder().unimorph(&tsv)` or `morph-fst new <code>` (see [Generating a language](#generating-a-language)); `--unimorph <tsv>` also works on `tokenize`, `analyze` and the other SOURCE commands. UniMorph's tables are CC BY-SA, so they are downloaded for you, never bundled.

For each lemma it picks a stem, splits every form into stem + suffix, and groups lemmas with the same suffixes into shared paradigm classes:
- **Stem choice.** The stem is the lemma prefix after which most forms end in a suffix that other lemmas show in the same cell.
- **Accent shifts** (`ór|ala` beside `or|ar`) become stem variants.
- **Stem changes** (`piens|an` beside `pens|ar`) also become stem variants, split before a known suffix.
- **Suppletive forms** (`went`) are listed whole.

The draft accepts exactly the input rows, each with its lemma and features. Spelling rules and harmony are not inferred, but stem shapes carry each class's stem changes to unseen lemmas, so the draft already generalizes in hybrid mode.

Measured on UniMorph data. Every tenth lemma is held out; the rest train the draft. Round trip: training forms analyzed with their lemma and features. Held-out: features of the top hybrid reading for forms of unseen lemmas, against a suffix guesser trained on the same rows.

| | Lemmas (train) | Forms | Classes | Listed whole | Round trip | Held-out: morph-fst | Held-out: suffix guesser |
|---|---|---|---|---|---|---|---|
| Turkish (first 4,000 lemmas) | 2,718 | 223,728 | 200 | 18 | 100% | **84.5%** | 74.7% |
| Spanish | 64,874 | 1,041,540 | 393 | 55 | 100% | 68.6% | 69.4% |
| English | 382,035 | 583,881 | 271 | 55 | 99.9%¹ | 77.4% | 78.0% |

¹ UniMorph lists `countable` as the plural of 734 lemmas; that one form has more readings than the evaluation's 512-analysis cap. Every other form round-trips.

The right features appear among the readings for 98.9% (Turkish), 97.7% (Spanish) and 96.2% (English) of held-out forms. Full English generates and compiles in 5.6 s into a 97 MB artifact (382k lemmas, mostly nouns).

### Discovering a grammar from a word list

For a language with no UniMorph table, `Lexer::from_wordlist(&text)` (or `--wordlist words.txt`, or `discover::from_wordlist` for the spec and the findings) finds the paradigms itself (`Lexer::builder().words(&text)` is the friendly door to it). Input is words, nothing else:

1. Every word is cut at every point into stem (3+ letters) and suffix (up to 6, or none).
2. A suffix is productive when at least `max(3, words/1000)` stems carry it beside another suffix.
3. Each word keeps the cut whose stem carries the most productive suffixes, the longest stem on a tie (`walk` beats `wal` for `walked`, because `walk` is also a word).
4. Stems with the same suffix set share a paradigm class. A stem seen with one suffix is not evidence, so its words are listed whole.

Measured as for UniMorph: forms of unseen lemmas, grammar built from the other lemmas' forms only (no lemmas, no features). The stem boundary the top hybrid reading proposes matches the true lemma boundary for 77% of English forms (4k-word list, 0.02 s to build). On Turkish it is 52% (200k words, 5 s): a suffix chain (`-lerimizden`) is split into its productive parts, but harmony variants (`lar`/`ler`) are still separate suffixes. Suffixes carry only their spelling (`SFX:ed`), no features. Treat it as a first draft: you get segmentation and stem classes, and you add features and rules by hand.

## Languages

| Spec | Shows | Examples (all in tests) |
|---|---|---|
| `english` | full open-class lexicon; spelling rules; derivation | unhappiness, stopped, happily, unbelievably |
| `turkish` | 2- and 4-way vowel harmony across suffix chains; d/t, c/ç; buffer y/s/n; softening; vowel drop; doubling; exceptional roots | evlerimizden, kitabı / kitapta, saatler, kalbi, geliyorum, gelmiyorum, geleceğim, burnu, hakkı |
| `spanish` | {S} plural; stem changes feeding c/qu, g/gu, z/c; accent loss; in- assimilation | canciones, lápices, piensan, empiece, almuerce, busqué, imposible, irreal |
| `french` | ç/ge before a/o; è/ll/tt stems; -al→-aux; feminine x/f/er; elision | mangeons, commençons, achète, appellerai, chevaux, heureusement, l'homme |
| `korean` | Hangul→jamo; 이/가, 을/를, 은/는, (으)로; 아/어 harmony and contraction; ㅡ ㅂ ㄷ 르 하 irregulars | 학생이, 서울로, 먹어요, 갔어요, 해요, 추워요, 몰라요, 들으면 |
| `igbo` (simplified, tones stripped) | ATR harmony in **both** directions | iri / ịga, inye / ịṅụ, eri / aga, riri / gara / hụrụ, erighị / agaghị |

Apart from English, these are compact demo lexicons (a few hundred morphemes each) that show the spec language. Extend them, or write your own.

**Normalization** is per language and applied by `analyze` and `analyze_text`:
- Latin combining marks are composed.
- Case folding is per language (Turkish `I→ı`, `İ→i`).
- `’` becomes `'` (and stays inside French words).
- `strip_tones` removes tone marks in any order while keeping dot below/above.
- Hangul is decomposed arithmetically into jamo, so a morpheme can end mid-syllable (가 + ᆻ = 갔). Output is recomposed for display.

## Performance

`morph-fst bench <lang>`: 100k words (50% grammatical, 25% typos, 25% random), one core, median of 11 runs. Wall-clock time on a shared machine varies by about ±10%.

| Words per second | Strict | Hybrid | WordPiece only |
|---|---|---|---|
| English (bundled) | 561 k | 36 k | 2.8 M |
| Turkish (bundled) | 720 k | 149 k | 5.3 M |

Hybrid mode searches for up to 16 ranked readings per unknown word, each with a borrowed stem shape. That makes it about 3× slower than a single-guess search, and it is what lifts held-out English from 24% to 77%. Known words cost the same in both modes.

The bundled English compiles in 330 ms into a 12.5 MB artifact, and has 29k stem shapes to search. Compile it once per process, or ship `to_bytes()`.

On the full UniMorph English grammar (382k lemmas, 276k stem shapes), strict mode takes 8 µs per word and hybrid 86 µs per unknown word.

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
- **No sentence context.** Some forms are ambiguous by spelling alone: `crackle` (noun or verb), `-ed` (past or participle), bare adjectives vs nouns. Hybrid mode returns all readings ranked; picking one needs context it does not see.
- **Demo-sized lexicons** for every language except English.
- **English readings tie by entry order.** ESDB gives no frequency per part of speech, so among equally short readings adjectives come first, then nouns, then verbs (the order that scored best). Lexicalized derivations ESDB lists as words stay whole: `making<N>` ranks before `make<V>+ing<PROG>`.
- **Korean segments are in jamo.** A boundary inside a syllable shows as partial jamo.
- **Punctuation** detection covers ASCII and the main Unicode punctuation blocks (`std` has no Unicode category tables).

## Layout

```
data/                 bundled .morph specs, Turkish sample corpus
src/spec/             parser + compiler: registers, archiphonemes, rules, guards, pins, pruning, zero morphemes
src/morphotactics.rs  lattice states, register tables, guards, pins, underlying-mode registers
src/shapes.rs         stem shapes: what unknown stems borrow from known ones
src/generate.rs       UniMorph → .morph drafts
src/discover.rs       word list → .morph drafts (paradigm discovery)
src/build.rs          LexerBuilder: combines tables, words, spec and corpus
src/word.rs           Word, Part, Kind: the everyday view of a result
tools/english/        hand-written English grammar
examples/build_english.rs   ESDB + grammar → data/english.morph
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

The English lexicon in `data/english.morph` is derived from ESDB, Copyright 2000-2026 by Kevin Atkinson, under a permissive MIT-like license; the full notice is in [data/english-ESDB-COPYRIGHT.txt](data/english-ESDB-COPYRIGHT.txt) and at the top of the spec.
