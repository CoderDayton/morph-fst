//! A tiny search index: documents and queries meet at the lemma.
//!
//!     cargo run --example search

use morph_fst::Lexer;
use std::collections::{BTreeMap, BTreeSet};

const DOCS: [&str; 4] = [
    "The children were running home before the storm.",
    "She ran a marathon last spring.",
    "Mice eat the farmers' stored grain.",
    "Meeting notes: the runners discussed training plans.",
];

fn main() -> morph_fst::Result<()> {
    let lx = Lexer::bundled("english")?;
    let mut index: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    for (id, doc) in DOCS.iter().enumerate() {
        for term in lx.terms(doc) {
            index.entry(term).or_default().insert(id);
        }
    }
    for query in ["run", "child", "mouse eat", "meet"] {
        let terms = lx.terms(query);
        let hits = DOCS.iter().enumerate().filter(|(id, _)| terms.iter().all(|t| index.get(t).is_some_and(|d| d.contains(id))));
        println!("{query:?} -> {terms:?}");
        for (_, doc) in hits {
            println!("    {doc}");
        }
    }
    Ok(())
}
