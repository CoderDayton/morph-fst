//! How well a generated grammar reads lemmas it never saw.
//!
//! `cargo run --release --example heldout -- <unimorph.tsv> [lemmas]` takes
//! `lemmas` evenly spaced lemmas (default 4000), holds out every tenth,
//! generates a grammar from the rest and reads each held-out form in hybrid
//! mode. A reading is right when it has the form's lemma and exact features.

use morph_fst::generate::from_unimorph;
use morph_fst::normalize::{compose_hangul, normalize};
use morph_fst::{Lexer, Mode, Segment, WordOutput};
use std::collections::{BTreeMap, BTreeSet};

type Feats = BTreeSet<String>;
type Reading = (String, Feats);

fn feats(s: &str) -> Feats {
    s.split(';').filter(|f| !f.is_empty()).map(|f| f.trim_start_matches('?').to_string()).collect()
}

fn key(lemma: &str) -> String {
    compose_hangul(lemma).to_lowercase()
}

fn reading(segs: &[Segment]) -> Reading {
    let lemma = match segs.first() {
        Some(Segment::Morph(m)) => m.lemma.clone(),
        Some(Segment::Unknown { lemma, .. }) => lemma.clone(),
        None => String::new(),
    };
    let tags = segs.iter().flat_map(|s| match s {
        Segment::Morph(m) => feats(&m.tag),
        Segment::Unknown { guess, .. } => feats(guess),
    });
    (lemma, tags.collect())
}

fn readings(out: &WordOutput) -> Vec<Reading> {
    match out {
        WordOutput::Analyzed { all, .. } => all
            .iter()
            .map(|a| (a.first().map(|m| m.lemma.clone()).unwrap_or_default(), a.iter().flat_map(|m| feats(&m.tag)).collect()))
            .collect(),
        WordOutput::Hybrid { all, .. } => all.iter().map(|r| reading(r)).collect(),
        WordOutput::Fallback(_) => Vec::new(),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: heldout <unimorph.tsv> [lemmas]")?;
    let want: usize = args.next().map_or(Ok(4000), |n| n.parse())?;
    let mut by_lemma: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for line in std::fs::read_to_string(path)?.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if let [lemma, form, tags, ..] = cols[..] {
            if !lemma.contains(' ') && !form.contains(' ') {
                by_lemma.entry(lemma.to_string()).or_default().push((form.to_string(), tags.to_string()));
            }
        }
    }
    let (mut train, mut test) = (String::new(), Vec::new());
    for (i, (lemma, rows)) in by_lemma.iter().step_by((by_lemma.len() / want).max(1)).enumerate() {
        for (form, tags) in rows {
            if i % 10 == 9 {
                test.push((lemma, form, tags));
            } else {
                train.push_str(&format!("{lemma}\t{form}\t{tags}\n"));
            }
        }
    }
    let generated = from_unimorph(&train)?;
    let lx = Lexer::compile(&generated.spec)?;
    let mut gold: BTreeMap<&str, Vec<(String, Feats)>> = BTreeMap::new();
    for (lemma, form, tags) in &test {
        gold.entry(form).or_default().push((key(lemma), feats(tags)));
    }
    let (mut top_feats, mut top_both, mut any_both) = (0, 0, 0);
    for (_, form, _) in &test {
        let found = readings(&lx.lex(&normalize(form, &lx.mt.norm), Mode::Hybrid));
        let right = |r: &Reading| gold.get(form.as_str()).is_some_and(|g| g.iter().any(|(l, f)| key(&r.0) == *l && r.1 == *f));
        let top = found.first();
        top_feats += top.is_some_and(|r| gold.get(form.as_str()).is_some_and(|g| g.iter().any(|(_, f)| r.1 == *f))) as usize;
        top_both += top.is_some_and(right) as usize;
        any_both += found.iter().any(right) as usize;
    }
    let pc = |n: usize| 100.0 * n as f64 / test.len().max(1) as f64;
    println!("train rows {}, classes {}, held-out forms {}", train.lines().count(), generated.paradigms, test.len());
    println!("top reading: features {:.1}%, lemma and features {:.1}%; in some reading {:.1}%", pc(top_feats), pc(top_both), pc(any_both));
    Ok(())
}
