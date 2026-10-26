# Changelog

## 0.1.0 (unreleased)

First release.

- `Lexer` (the fused lexer): one `.morph` spec → one compile loop → one
  artifact → one left-to-right scan per word. It gives a full morphological
  analysis, a deterministic WordPiece (LinMaxMatch) fallback, or a hybrid
  output (known affixes around one unknown stem).
- Spec language:
  - lexicons with continuation classes and next-morpheme filters;
  - sequential rewrite rules;
  - archiphonemes;
  - left (`FEATURE`) and right (`RFEATURE`) context registers for
    long-distance harmony in both directions;
  - `OPTION harmony underlying` for opaque (non-surface-true) harmony, with
    `@tier=` overrides;
  - lexical exceptions via `@set:` / `@rset:`.
- Exception learning: `INDUCE <features> IN <lexicons>` plus
  `Lexer::compile_with_corpus` / the `induce` module learn which roots are
  exceptions from raw text. Hybrid mode infers an unknown stem's exceptions.
- Bundled demo specs: English, Turkish, Spanish, French, Korean, Igbo.
- Validated binary artifacts (`to_bytes` / `from_bytes`, format version 3).
- Feature `cli`: the `morph-fst` command-line tool.
- Minimum supported Rust version: 1.82.
