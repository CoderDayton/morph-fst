# Guide

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
| Running text | `lx.analyze_text(text)`: [tokenized](#tokenizer), each word normalized (Turkish `I→ı`, tone stripping, Hangul → jamo, …), and spread over all cores when long (each distinct word is lexed once). `word.span()` points into your text |
| Just the tokens | `morph_fst::tokenize(text)`: spans and classes, no lexicon needed |
| Tokens of one kind | `lx.split(text, how)`: `Split::Words` (as written), `Split::Morphemes` (`un happi est`), `Split::Lemmas` (search keys) or `Split::Pieces` (WordPiece-style pieces for a model: known morphemes whole, `un ##happi ##est`; unknown stems fall back to the grammar's pieces, then bytes) |
| Search keys | `lx.terms(text)`, the same as `Split::Lemmas`: one lowercase lemma per word, for documents and queries alike, see [Search](#search) |
| Rank documents for a query | `lx.index(docs)`, then `index.search(query)` → `(doc id, score)` best first, `index.score(query, id)`, `index.add(doc)`: BM25 over lemmas, see [Search](#search) |
| Skip the guessing | `Lexer::bundled("english")?.mode(Mode::Strict)` |
| Everything about a word | `word.detail()`: byte spans, every reading, subword pieces |
| Save and reload | `lx.save(path)`, `Lexer::load(path)` (the saved file keeps the mode) |

A `Word` has `text()`, `span()`, `kind()`, `lemma()` (the whole word's dictionary form: `runners` → `runner`, `really` stays `really`), `parts()`, `alternatives()` (other readings, best first), `tokens()` and `Display`. A `Part` has `text()`, `lemma()`, `tag()` and `is_known()`.

Runnable examples:

```sh
cargo run --example quickstart
cargo run --example search
cargo run --example learn_exceptions
```

## Search

Use `lx.terms(text)` on both the documents and the queries, and `run`, `runs`, `ran` and `running` meet at one key (`cargo run --example search`). Numbers, emails, URLs and abbreviations stay whole; punctuation, symbols and clitics are dropped; unknown words and names still get a key. Derived words keep their own key (`runner` ≠ `run`), because the lemma keeps derivation. How it compares with a stemmer is in [Results](results.md#search-keys).

`lx.index(docs)` does the ranking too. It scores each document for a query with BM25 over those keys, at the published defaults (k1 = 1.2, b = 0.75): a rare shared word counts more than a common one, a repeat counts less each time, and a long document does not win by length alone.

```rust
let index = lx.index(["The cat ran home.", "Dogs are running in the park."]);
index.search("dog runs");      // [(1, 1.25), (0, 0.49)]: both share `run`, only 1 shares `dog`
```

## Tokenizer

`tokenize` is lossless: every non-space character is in exactly one token, only whitespace lies between tokens, and spans index your original text. Rules run in a fixed order with no tuned weights: URL, email, initialism (`U.S.`), then word or number, symbol, punctuation. English clitics split off (`Smith|'s`, `do|n't`); other inner apostrophes stay (`o'clock`); hyphenated words split at the hyphen.

```text
Dr. | Smith | 's | e | - | mail | : | x@y.com | costs | $ | 4.50 | ! | Do | n't | https://a.b/c?d=1 | e.g. | U.S. | ... | 1,000 | ( | 3rd | )
```

The abbreviation list and clitics are English. Other languages get the generic rules; `apostrophe_in_words` languages never split at an apostrophe.

## Generating a language

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
| A Hunspell dictionary (`.hunspell(&aff, &dic)`) | stems and endings from its affix rules; the affix classes stand in for features |
| A word list only | stems and endings (no features), found by [paradigm discovery](#from-a-word-list) |
| Tables and a word list | the tables' grammar, plus paradigms discovered from the words the tables reject |
| A spec | exactly your grammar; a word list or corpus adds to it |

From the command line one command does it, including the download:

```sh
morph-fst new rus -o russian.mcfu                          # fetches UniMorph's Russian table
morph-fst new rus -o russian.mcfu --wordlist words.txt     # plus your words
morph-fst new -o spanish.mcfu --hunspell es_ES.aff         # from a spell-checker (es_ES.dic beside it)
morph-fst tokenize --fused russian.mcfu "я люблю читать книги"
```

UniMorph tables and UD treebanks are CC BY-SA, so they are downloaded for you, never bundled.

### From UniMorph tables

[UniMorph](https://unimorph.github.io) tables (`lemma<TAB>form<TAB>FEAT;FEAT;…`, 180+ languages) give a starting point:

```sh
morph-fst generate spa.tsv -o spanish-draft.morph
```

To skip the spec file, use `Lexer::builder().unimorph(&tsv)` or `morph-fst new <code>`; `--unimorph <tsv>` also works on `tokenize`, `analyze` and the other SOURCE commands.

For each lemma it picks a stem, splits every form into stem + suffix, and groups lemmas with the same suffixes into shared paradigm classes:
- **Stem choice.** The stem is the lemma prefix after which most forms end in a suffix that other lemmas show in the same cell.
- **Accent shifts** (`ór|ala` beside `or|ar`) become stem variants.
- **Stem changes** (`piens|an` beside `pens|ar`) also become stem variants, split before a known suffix.
- **Suppletive forms** (`went`) are listed whole.

The draft accepts exactly the input rows, each with its lemma and features. Spelling rules and harmony are not inferred, but stem shapes carry each class's stem changes to unseen lemmas, so the draft already generalizes in hybrid mode. A UniMorph grammar has no function words (the tables list only words that inflect); a treebank can add them, see [learning context rules](grammar.md#learning-rules-from-a-treebank).

### From a word list

For a language with no UniMorph table, `Lexer::from_wordlist(&text)` (or `--wordlist words.txt`, or `discover::from_wordlist` for the spec and the findings) finds the paradigms itself (`Lexer::builder().words(&text)` is the friendly door to it). Input is words, nothing else:

1. Every word is cut at every point into stem (3+ letters) and suffix (up to 6, or none).
2. A suffix is productive when at least `max(3, words/1000)` stems carry it beside another suffix.
3. Each word keeps the cut whose stem carries the most productive suffixes, the longest stem on a tie (`walk` beats `wal` for `walked`, because `walk` is also a word).
4. Stems with the same suffix set share a paradigm class. A stem seen with one suffix is not evidence, so its words are listed whole.

Suffixes carry only their spelling (`SFX:ed`), no features. Treat it as a first draft: you get segmentation and stem classes, and you add features and rules by hand.

## Command line

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

## Cargo features

| Feature | What it adds |
|---|---|
| *(default)* | `Lexer`, the spec compiler, exception learning, bundled languages. |
| `cli` | The `morph-fst` binary. |

## Bundled languages

| Spec | Shows | Examples (all in tests) |
|---|---|---|
| `english` | full open-class lexicon; spelling rules; derivation | unhappiness, stopped, happily, unbelievably |
| `turkish` | 2- and 4-way vowel harmony across suffix chains; d/t, c/ç; buffer y/s/n; softening; vowel drop; doubling; exceptional roots | evlerimizden, kitabı / kitapta, saatler, kalbi, geliyorum, gelmiyorum, geleceğim, burnu, hakkı |
| `spanish` | {S} plural; stem changes feeding c/qu, g/gu, z/c; accent loss; in- assimilation | canciones, lápices, piensan, empiece, almuerce, busqué, imposible, irreal |
| `french` | ç/ge before a/o; è/ll/tt stems; -al→-aux; feminine x/f/er; elision | mangeons, commençons, achète, appellerai, chevaux, heureusement, l'homme |
| `korean` | Hangul→jamo; 이/가, 을/를, 은/는, (으)로; 아/어 harmony and contraction; ㅡ ㅂ ㄷ 르 하 irregulars | 학생이, 서울로, 먹어요, 갔어요, 해요, 추워요, 몰라요, 들으면 |
| `igbo` (simplified, tones stripped) | ATR harmony in **both** directions | iri / ịga, inye / ịṅụ, eri / aga, riri / gara / hụrụ, erighị / agaghị |

English, Turkish, Spanish, French and Korean carry full lexicons, generated from a hand-written grammar plus third-party data (`examples/build_<language>.rs`; sources and licenses in the README). Igbo is a compact demo lexicon. Small demo grammars for the other four, in `tests/fixtures/`, are what the tests enumerate exhaustively. How the English one is built is in [How it works](how-it-works.md#the-bundled-english).

**Normalization** is per language and applied by `analyze` and `analyze_text`:
- Latin combining marks are composed.
- Case folding is per language (Turkish `I→ı`, `İ→i`).
- `’` becomes `'` (and stays inside French words).
- `strip_tones` removes tone marks in any order while keeping dot below/above.
- `strip_vowel_marks` removes Hebrew niqqud and cantillation, Arabic harakat and tatweel. A Hebrew final letter (`ך ם ן ף ץ`) matches its medial form inside a stem, and a guessed lemma is spelled with the final form.
- Hangul is decomposed arithmetically into jamo, so a morpheme can end mid-syllable (가 + ᆻ = 갔). Output is recomposed for display.
