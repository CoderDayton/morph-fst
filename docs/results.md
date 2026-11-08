# Results

Every number here was measured on data the lexer was not built from. UniMorph, UD and Hunspell data are CC BY-SA or carry their own licences, so they are fetched for the check, never bundled.

## Real English text

Scored on the 25k tokens of the [UD English EWT](https://github.com/UniversalDependencies/UD_English-EWT) test set:

| Check | Result |
|---|---|
| Token boundaries (exact span, F1) | 97.1% |
| Lemma of the best reading (20.1k alphabetic words) | 96.7% |
| Part of speech of the best reading in `analyze_text` (N/V/ADJ/ADV/DET and closed classes, 20.1k words) | 87.7% |

### Part of speech in context

Part of speech is still the weakest check. Spelling alone gets 81.2%. The grammar's `PREFER` rules ([the grammar language](grammar.md#context-prefer)) then pick a reading from the neighbouring words, which brings it to 87.7%. Six rules are hand-written (a verb after `to` or a pronoun, a noun after a determiner). 32 more were learned from the EWT training set and stopped on dev, and the score is on test. `analyze` sees one word, so it keeps the context-free order. Reproduce with `cargo run --release --example treebank -- score en_ewt-ud-test.conllu`.

On German (UniMorph `deu`, UD German-GSD test), a grammar built from the tables alone has no function words:

| German setup | Part of speech | Lemma |
|---|---|---|
| UniMorph grammar alone | 45.4% | 23.6% |
| + the treebank's function words (`--closed`) | 83.0% | 62.3% |
| + 14 learned `PREFER` rules | 87.3% | 65.9% |

### Search keys

How often forms of the same word share a key (`lx.terms`), over the 3.6k distinct word forms of the UD EWT test set, scored pairwise against its lemmas (recall: forms of one word that meet; precision: forms that meet that really are one word):

| Key | Recall | Precision | F1 |
|---|---|---|---|
| Porter2 stem | 65.0% | 57.8% | 61.2% |
| **morph-fst lemma** | **82.1%** | **78.8%** | **80.4%** |
| The treebank's own lemma | 92.7% | 88.1% | 90.4% |

This scores word-form conflation, not retrieval: no queries or relevance judgments were used.

## Real text in the other bundled languages

Each bundled grammar is built from a permissively licensed lexicon (see its `examples/build_<lang>.rs`) and scored with `analyze_text` on a UD test set it was not built from. Before is the earlier demo-sized grammar.

| Language | Lexicon (licence) | UD test set | Words | Part of speech | Lemma |
|---|---|---|---|---|---|
| Spanish | RLA-ES Hunspell (MPL-1.1) | AnCora | 36,280 | 57.3% → **88.4%** | 56.9% → **94.9%** |
| French | Grammalecte (MPL-2.0) | GSD | 5,948 | 56.1% → **87.2%** | 61.2% → **89.2%** |
| Turkish | Zemberek-NLP (Apache-2.0) | BOUN | 9,239 | 55.5% → **80.3%** | 56.5% → **85.7%** |

Korean (mecab-ko-dic, Apache-2.0) is scored on UD Korean-Kaist by stem class, since Kaist lemmas are morpheme sequences rather than one dictionary form: of 23,875 words, 77.8% are now known to the grammar (2.7% before) and the stem's part of speech is right for 76.4% (58.9% before). Most remaining Korean misses are verbs read as adjectives (`하다` words listed as both).

Turkish context rules add 2.3 points of part of speech (78.0% → 80.3%).

## Unseen words

On English (UniMorph, 584k forms, 382k lemmas), every tenth lemma was held out and all its forms run in hybrid mode. These are words the analyzer has never seen:

| 65,327 forms of unseen lemmas | Analyzed | Features, top reading | Lemma, top reading | Features in some reading |
|---|---|---|---|---|
| Dictionary lookup | 0% | 0% | 0% | 0% |
| Suffix guesser trained on the same table | 100% | 78.0% | 91.6% | 91.9% |
| **morph-fst hybrid** | 100% | 77.4% | 90.7% | **96.2%** |

The suffix guesser is the standard statistical baseline (longest seen ending → most frequent tag and lemma edit). morph-fst matches it on the top reading without counting anything, and it also returns the segmentation, a lemma produced by the spelling rules, and the right reading among its candidates 96% of the time. Most of the remaining top-1 misses can't be fixed without sentence context: noun vs adjective for bare words, past vs participle for `-ed`.

### The bundled English

Scored on all 649,583 UniMorph English forms. Most of their lemmas are not in the bundled lexicon.

| Bundled English | Entries | Lookup: features | Hybrid: features | Hybrid: lemma | Features in some reading |
|---|---|---|---|---|---|
| Demo grammar | 442 | 0.5% | 45.1% | 80.4% | 85.1% |
| Hand-written word lists | 4,819 | 3.6% | 67.2% | 90.5% | 96.6% |
| **Generated from ESDB** | 57,788 | **19.0%** | **74.7%** | **92.8%** | 95.3% |

### Grammars drafted from UniMorph

Every tenth lemma is held out; the rest train the draft. Round trip: training forms analyzed with their lemma and features. Held-out: features of the top hybrid reading for forms of unseen lemmas, against a suffix guesser trained on the same rows.

| | Lemmas (train) | Forms | Classes | Listed whole | Round trip | Held-out: morph-fst | Held-out: suffix guesser |
|---|---|---|---|---|---|---|---|
| Turkish (first 4,000 lemmas) | 2,718 | 223,728 | 200 | 18 | 100% | **84.5%** | 74.7% |
| Spanish | 64,874 | 1,041,540 | 393 | 55 | 100% | 68.6% | 69.4% |
| English | 382,035 | 583,881 | 271 | 55 | 99.9%¹ | 77.4% | 78.0% |

¹ UniMorph lists `countable` as the plural of 734 lemmas; that one form has more readings than the evaluation's 512-analysis cap. Every other form round-trips.

The right features appear among the readings for 98.9% (Turkish), 97.7% (Spanish) and 96.2% (English) of held-out forms. Full English generates and compiles in 5.6 s into a 97 MB artifact (382k lemmas, mostly nouns). Where suffix chains and harmony matter, the grammar pulls ahead of the guesser: 84.5% against 74.7% on Turkish.

### Across 18 languages

Same protocol for every language (`cargo run --release --example heldout -- <tsv>...`: 4,000 evenly spaced lemmas, every tenth held out, a grammar built from the rest, each held-out form read in hybrid mode). Right means the top reading has the form's lemma and exact features; the last column counts a hit anywhere in the readings. All 18 run in about 40 s. `--baseline examples/heldout.baseline` prints each change against these numbers and fails on any drop.

| Language | Top: features | Top: lemma + features | Any reading |
|---|---|---|---|
| Portuguese | 96.3% | 95.3% | 99.2% |
| Italian | 98.5% | 93.7% | 98.9% |
| Turkish | 94.5% | 90.7% | 97.9% |
| French | 97.1% | 87.7% | 94.2% |
| Hungarian | 90.9% | 86.5% | 97.7% |
| Spanish | 90.4% | 84.9% | 96.5% |
| Czech | 89.5% | 83.6% | 95.0% |
| Finnish | 91.8% | 81.5% | 96.1% |
| Russian | 88.7% | 81.0% | 96.0% |
| Dutch | 86.3% | 79.6% | 93.1% |
| English | 80.0% | 79.1% | 98.8% |
| Georgian | 89.9% | 78.4% | 91.7% |
| Korean | 80.3% | 77.9% | 87.1% |
| Polish | 88.0% | 77.1% | 93.6% |
| Swedish | 80.7% | 77.0% | 98.1% |
| German | 65.1% | 63.4% | 84.1% |
| Hebrew | 81.2% | 52.5% | 88.1% |
| Arabic | 30.6% | 19.8% | 73.0% |

The gap between the last two columns is ranking: the right reading is there but not first (Swedish, English). The gap between the first two is lemma rebuilding: the features are right but the lemma is not (Hebrew, French, Finnish). Two ways of narrowing it were measured across all 18 languages and lost, so neither is done: ranking tied readings by the size of their class, and allowing any one-letter stem change (Finnish gradation is not one letter at a time).

Three changes lifted the bottom of the table:
- Participles, converbs and masdars (`V.PTCP`, `V.CVB`, `V.MSDR`) join their verb's paradigm instead of forming lexemes of their own (Korean, Arabic, Russian).
- Vowel points are stripped when the tables carry them (Hebrew, Arabic).
- A stem shape now records a change at the start of the stem as well as at the end, and hybrid mode reads it on unseen stems:
  - a prefix (German `ge|kauf|t` → `kaufen`, Hebrew `מ|סביר` → `הסביר`);
  - one marked letter (Spanish `trángala` → `trangar`);
  - a capital (German `torheiten` → `Torheit`).

Suffixing languages with regular stems still do best. The remaining weak spots:
- **Templatic morphology (Arabic, Hebrew).** The pattern's prefix is read now, but the vowels woven between root letters are not. Unvocalized text leaves active and passive (or several binyanim) spelled the same, so the right reading is often present but not first.
- **Matres lectionis (Hebrew `י`/`ו` inserted inside the stem).** One inserted or dropped letter per stem was tried and cost Hebrew and Turkish accuracy, so it is not done.
- **Syncretism (German, Russian, Polish).** Case endings that several cells share leave readings tied.
- **Korean.** Many misses are `하다` words listed as both adjective and verb, which spelling cannot separate.

### Grammars from a spell-checker or a word list

- **Hunspell alone**, scored on every 20th UniMorph form: Spanish `es_ES` (679k rows, built in 4 s) gets 69.7% of lemmas right; German `de_DE_frami` gets 37.6%, because it lists many inflected forms as stems of their own.
- **A plain word list** (no lemmas, no features): the stem boundary the top hybrid reading proposes matches the true lemma boundary for 77% of English forms (4k-word list, 0.02 s to build). On Turkish it is 52% (200k words, 5 s): a suffix chain (`-lerimizden`) is split into its productive parts, and harmony variants (`lar`/`ler`: same consonants, never on the same stem) share one tag, though they stay separate endings in the grammar, so accuracy does not change.

## Speed

`morph-fst bench <lang>`: 100k words (50% grammatical, 25% typos, 25% random), one core, median of 11 runs. Wall-clock time on a shared machine varies by about ±10%.

| Words per second | Strict | Hybrid | WordPiece only |
|---|---|---|---|
| English (bundled) | 561 k | 36 k | 2.8 M |
| Turkish (bundled) | 720 k | 149 k | 5.3 M |

Hybrid mode searches for up to 16 ranked readings per unknown word, each with a borrowed stem shape. That makes it about 3× slower than a single-guess search, and it is what lifts held-out English from 24% to 77%. Known words cost the same in both modes. On 18k unseen English words, guessing takes 881 ms on one thread and 165 ms on 32 cores.

The bundled English compiles in 325 ms into a 13.0 MB artifact, and has 29k stem shapes to search. Compile it once per process, or ship `to_bytes()`.

`Lexer::bundled` compiles on every call. Measured on one machine, against reloading a saved artifact (`save` once, then `load`):

| Bundled | Compile | Reload |
|---|---|---|
| English | 0.36 s | 0.06 s |
| Turkish | 0.32 s | 0.03 s |
| French | 0.92 s | 0.17 s |
| Spanish | 1.38 s | 0.17 s |
| Korean | 1.63 s | 0.29 s |

On the full UniMorph English grammar (382k lemmas, 276k stem shapes), strict mode takes 8 µs per word and hybrid 86 µs per unknown word.

## How it is tested

Run the full suite with `cargo test --release`.

- **Independent oracle** (`src/engine/oracle.rs`).
  - It is a separate interpreter of the spec AST, with its own register simulation, archiphoneme resolution (branch-and-verify for right context, tier-based for underlying mode) and rule matcher.
  - For every language, the lexer must accept exactly the surfaces the oracle generates (up to 4 morphemes) with exactly the same glosses. It must also reject every rule-less form and every wrong archiphoneme choice: about 40k surfaces and 150k forbidden forms.
- **Hybrid ≡ strict** on every word strict mode accepts: 25k random words per language, 5k–8k per toy spec.
- **Mutation checks.** Each of these makes the tests fail:
  - disabling the guard check;
  - disabling left pins or right pins;
  - ignoring the right register;
  - freezing the underlying-mode register.
- **LinMaxMatch** is checked against a naive greedy reference on 1,500 random vocabularies.
- **Contracts** (`tests/contracts.rs`): alternatives come back in true rank order, a repeated entry changes nothing, a saved lexer keeps its mode, and extending a grammar keeps its meaning.
- **Robustness.**
  - The fallback is lossless on random Unicode.
  - 1 MB words are handled.
  - Corrupted artifacts give `Err`, never a panic. This includes crafted ones re-signed with a valid checksum (12k per run; set `HOSTILE_N` for more). Any that load must then lex without panicking, and a loaded artifact's subword pieces must spell their trie paths.
  - Builds are deterministic.
