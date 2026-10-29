# morph-fst

Rust crate: `.morph` spec → one fused artifact → one scan per word. `Lexer` is the only public entry point. The grammar is the model: no training, no tuned weights.

## Commands

```sh
cargo test --release --all-features
cargo clippy --release --all-features --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo run --release --example build_english -- <ESDB scowl-pre.txt> > data/english.morph
```

## Router

| Touching | Look at |
|---|---|
| Spec syntax, rules, registers | `src/spec/` |
| Lattice states, guards, pins | `src/morphotactics.rs` |
| Scan, hybrid search, artifact | `src/fused/` |
| Stem shapes (unknown stems) | `src/shapes.rs` |
| Subword trie, LinMaxMatch | `src/subword/` |
| Everyday API (`analyze`, `Word`, `Part`, `Kind`) | `src/word.rs`, `src/fused/mod.rs` |
| Language builder (`Lexer::builder`) | `src/build.rs` |
| UniMorph → spec | `src/generate.rs` |
| Word list → spec | `src/discover.rs` |
| Exception learning | `src/induce.rs` |
| English data | `tools/english/grammar.morph` + `examples/build_english.rs`; never hand-edit `data/english.morph` |
| Reference interpreter | `src/oracle.rs` (tests only) |

## Guards

- `data/english.morph` is generated. Edit the grammar, then regenerate.
- No `unwrap`, `expect`, `unreachable`, `unsafe`: the loader must return `Err` on any bad artifact (`tests/hostile_artifacts.rs`).
- Hybrid must equal strict on every word strict accepts.
- Ranking is lexicographic, never weighted. Do not add tuned constants.
- Dependencies are fine when lightweight and already written. Do not hand-roll what a small crate does well.
- Perf changes need before/after numbers on the same word list; revert no-gain changes.
- UniMorph/Wiktextract data is CC BY-SA: fetch it, never bundle it.
- Public API is `Lexer`, `LexerBuilder`, `Word`, `Part`, `Kind`, `Mode`, `Error`. Keep it small and plain-worded; new features go behind the builder, not as new constructors. `lex*` and `WordOutput` are the precise layer (`Word::detail`).
- Public API changes (new methods, artifact format) need approval.
