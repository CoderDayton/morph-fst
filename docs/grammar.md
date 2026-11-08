# The grammar language

A `.morph` file describes one language: its stems, the endings each kind of stem takes (`LEXICON` sections that point to the next one), the spelling rules between them (`RULES`), and optional context such as vowel harmony. `tools/english/grammar.morph` and the files in `data/` are full examples.

## At a glance

```
OPTION case turkish|lower|none        OPTION hangul_jamo true      OPTION apostrophe_in_words true
OPTION strip_tones true               ! drop tone marks, keep dot below/above (Igbo ị́ → ị)
OPTION harmony surface|underlying     ! what harmony reads (default: surface)
OPTION induce_min_count 2             ! evidence needed to learn an exception
OPTION derivation PFX,NMLZ,AGENT      ! tags that are derivation, not inflection (`Word::lemma` keeps them)
OPTION strip_vowel_marks true         ! drop Hebrew niqqud and Arabic harakat (set by the generator when the tables use them)
OPTION prefer_inflection false        ! keep a listed word whole instead of ranking stem + ending above it (default: true)
PREFER V,MOD AFTER MOD,"to"           ! context: see below
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

## Long-distance context, both directions

- **`FEATURE`: left context.**
  - It is a small deterministic automaton over the characters before a position: "backness/rounding of the last vowel", "is the last consonant voiceless".
  - Archiphonemes (`l{A}r`, `{D}{A}n`) are realized **ahead of time** for every register value and grouped into concrete allomorphs (`lar`/`ler`), each guarded by a bitset.
  - At runtime the register costs one table lookup per character and one bit test per arc. It adds no lattice states.
- **`RFEATURE`: right context.**
  - It is the same automaton, run backward from the end of the word, for prefixes that agree with what follows (Igbo i-/ị-) and for dominant harmony.
  - It costs one extra O(n) pass, only for languages that declare it.
  - Archiphonemes can read left, right or both. Guards become bitsets over (left, right) register pairs.

## Context: `PREFER`

```
PREFER N,ADJ AFTER DET,"my"           ! after a determiner or `my`, take a noun or adjective reading
PREFER ADJ OVER ADV BEFORE N          ! only an ADV best reading gives way, and only before a noun
```

In `analyze_text`, a word whose best reading does not match the preferred tags takes its first alternative that does. This happens only when the word before (`AFTER`) or after (`BEFORE`) matches. A reading's tag is its last derivation or its stem (`V` for `make+ing`), and a tag also matches its `TAG:sub` forms. `"x"` matches a lemma and `'x'` the word as written, so `'him'` can stand apart from `'he'` where the lemma `"he"` covers both. The word before is read as already chosen, the word after by its best reading. The first rule that applies decides (its neighbour matches, and so do its `OVER` tags if it has any), so rules are ordered, never weighted. The displaced reading stays first among the alternatives.

### Learning rules from a treebank

Rules can be learned from a [UD treebank](https://universaldependencies.org) (CC BY-SA; fetched, never bundled). Each round of `cargo run --release --example treebank -- learn train.conllu dev.conllu [--spec F | --unimorph T]` adds the rule that fixes the most training words net of those it breaks, and learning stops at the first rule that does not help dev. The output is `PREFER` lines to paste into the grammar.

A UniMorph grammar has no function words, because the tables list only words that inflect. `--closed train.conllu` adds the determiners, pronouns, adpositions, conjunctions and adverbs the treebank shows at least twice. The effect on English and German is in [Results](results.md#part-of-speech-in-context).

## Opaque harmony: `OPTION harmony underlying`

By default harmony is **surface-true**: registers read the final text. In many languages that is wrong:
- a rule changes a vowel, but the suffixes still harmonize with the *original* vowel;
- a root behaves as if it had different vowels.

With `OPTION harmony underlying`:

- Each morpheme contributes its **pre-rule form** to the harmony tier. Rules still decide the surface, but harmony no longer sees their effect.
- `@tier=xyz` gives a morpheme an abstract tier form. For example `pil @tier=pul` is a root spelled with a front vowel that harmonizes as back.
- The registers move into the lattice state. They are carried morpheme by morpheme instead of being read off the surface. Right context uses guess-and-verify with an inverse table, so analyses never duplicate.
- An unknown stem in hybrid mode contributes its own letters to the tier.

Example (`tests/exceptions.rs`): `ï` is front, but a rule turns it into `a` before the plural. The plural still harmonizes with `ï`, so `kït+lAr → katler` (not `*katlar`), and the prefix agrees with the underlying root: `mikatler`.

## Lexical exceptions, learned instead of marked

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
