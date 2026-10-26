//! The single compile loop.

use std::collections::{BTreeMap, BTreeSet};

use super::FusedLexer;
use crate::morphotactics::Morphotactics;
use crate::spec::{self, ClassId, Grammar};
use crate::subword::{closed_piece_set, PieceDef, TrieBuilder};

/// Derive the WordPiece vocabulary from the grammar itself:
/// morphemes of lexicons that can start a word → initial pieces; morphemes of
/// lexicons reachable after another morpheme → `##` pieces (stems are both);
/// plus the PIECES section; closed under single characters.
pub fn derive_vocab(g: &Grammar) -> Vec<PieceDef> {
    let initial: BTreeSet<ClassId> = g.class_closure(g.root).into_iter().collect();
    let closures: Vec<Vec<ClassId>> = (0..g.classes.len()).map(|c| g.class_closure(c as ClassId)).collect();
    let mut after: BTreeSet<ClassId> = BTreeSet::new();
    loop {
        let mut changed = false;
        for e in &g.entries {
            if initial.contains(&e.lexicon) || after.contains(&e.lexicon) {
                for &c in &closures[e.cont as usize] {
                    changed |= after.insert(c);
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut raw: Vec<(&str, bool)> = Vec::new();
    for e in &g.entries {
        let ini = initial.contains(&e.lexicon);
        let aft = after.contains(&e.lexicon);
        if ini || (aft && g.options.suffix_initial_pieces) {
            raw.push((&e.surface, false));
        }
        if aft {
            raw.push((&e.surface, true));
        }
    }
    raw.extend(g.extra_pieces.iter().map(|(t, c)| (t.as_str(), *c)));
    closed_piece_set(raw).into_iter().map(|(cont, text)| PieceDef { text, cont }).collect()
}

#[derive(Default)]
struct KeyAttrs {
    piece: bool,
    payload: Vec<u32>,
}

impl FusedLexer {
    /// Spec text → fused lexer, in one pass.
    pub fn compile(spec_text: &str) -> crate::Result<Self> {
        Ok(Self::from_grammar(&spec::load(spec_text)?))
    }

    /// Compile a spec after learning its lexical exceptions (`INDUCE`) from
    /// raw `corpus` text. Without INDUCE this is [`Self::compile`].
    pub fn compile_with_corpus(spec_text: &str, corpus: &str) -> crate::Result<(Self, crate::induce::Induction)> {
        let ind = crate::induce::induce(&spec::parse_spec(spec_text)?, corpus)?;
        let lx = Self::from_grammar(&spec::compile_grammar(&ind.spec)?);
        Ok((lx, ind))
    }

    /// Build from an already compiled [`Grammar`] (see [`spec::load`]).
    pub fn from_grammar(g: &Grammar) -> Self {
        let mt = Morphotactics::from_grammar(g);
        // Unified key stream: (is_cont_root, key) -> {piece?, morpheme ids}.
        let mut keys: BTreeMap<(bool, &str), KeyAttrs> = BTreeMap::new();
        let vocab = derive_vocab(g);
        for p in &vocab {
            keys.entry((p.cont, p.text.as_str())).or_default().piece = true;
        }
        for (id, e) in g.entries.iter().enumerate() {
            keys.entry((false, e.surface.as_str())).or_default().payload.push(id as u32);
        }
        // ONE insertion loop ...
        let mut tb = TrieBuilder::new();
        for ((cont, key), a) in &keys {
            tb.add_key(key, *cont, a.piece, &a.payload);
        }
        // ... and ONE BFS (failure links, pops, CSR, payload packing).
        FusedLexer { mt, trie: tb.finish() }
    }
}
