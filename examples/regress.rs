//! English no-regression harness: pinned corpus, median-of-N throughput,
//! and byte-level fingerprints of everything that must not change.
//! usage: cargo run --release --example regress -- <outdir> [reps]
use morph_fst::codec::{fnv1a64, Writer};
use morph_fst::fused::derive_vocab;
use morph_fst::{Lexer, Mode};
use morph_fst::rng::Rng;
use morph_fst::{oracle, spec, WordOutput};
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let dir = args.next().ok_or("usage: regress <outdir> [reps]")?;
    let reps: usize = args.next().map(|s| s.parse()).transpose()?.unwrap_or(11).max(1);
    let src = include_str!("../data/english.morph");
    let corpus_path = format!("{dir}/corpus.txt");
    let words: Vec<String> = match std::fs::read_to_string(&corpus_path) {
        Ok(t) => t.lines().map(String::from).collect(),
        Err(_) => {
            let good: Vec<String> = oracle::generate(&spec::parse_spec(src)?, 4, 2_000_000)?.surfaces.into_keys().collect();
            let alpha: Vec<char> = "abcdefghijklmnopqrstuvwxyz".chars().collect();
            let mut rng = Rng::new(7);
            let w: Vec<String> = (0..100_000)
                .map(|i| match i % 4 {
                    0 | 1 => rng.pick(&good).clone(),
                    2 => {
                        let mut cs: Vec<char> = rng.pick(&good).chars().collect();
                        let k = rng.below(cs.len());
                        cs[k] = *rng.pick(&alpha);
                        cs.into_iter().collect()
                    }
                    _ => (0..3 + rng.below(10)).map(|_| *rng.pick(&alpha)).collect(),
                })
                .collect();
            std::fs::write(&corpus_path, w.join("\n"))?;
            w
        }
    };
    let g = spec::load(src)?;
    let fused = Lexer::compile(src)?;

    // ---- fingerprints ----
    let mut dump = String::new();
    for e in &g.entries {
        let f = &g.filters[e.filter as usize];
        dump.push_str(&format!("{} {} {} {} {} {:?} {:?} {:?}\n", e.lexicon, e.surface, e.lex_surface, e.lemma, e.tag, e.cont, (f.kind as u8, &f.ids), e.rule));
    }
    let vocab: Vec<String> = derive_vocab(&g).iter().map(|p| format!("{}{}", if p.cont { "##" } else { "" }, p.text)).collect();
    let mut w = Writer::new();
    fused.trie.write(&mut w);
    let trie = w.into_inner();
    let mut lat = String::new();
    let mut outs = String::new();
    for word in &words {
        match fused.lattice(word) {
            Some(l) => lat.push_str(&format!("{} {}\n", l.states.iter().map(|s| s.len()).sum::<usize>(), l.edges.len())),
            None => lat.push_str("-\n"),
        }
        outs.push_str(&format!("{:?}\n", fused.lex(word, Mode::Hybrid)));
    }
    std::fs::write(format!("{dir}/dump.txt"), &dump)?;
    std::fs::write(format!("{dir}/vocab.txt"), vocab.join("\n"))?;
    std::fs::write(format!("{dir}/trie.bin"), &trie)?;
    std::fs::write(format!("{dir}/lattice.txt"), &lat)?;
    std::fs::write(format!("{dir}/outputs.txt"), &outs)?;
    println!("fingerprints: dump {:016x} vocab {:016x} trie {:016x} lattice {:016x} outputs {:016x}",
        fnv1a64(dump.as_bytes()), fnv1a64(vocab.join("\n").as_bytes()), fnv1a64(&trie), fnv1a64(lat.as_bytes()), fnv1a64(outs.as_bytes()));

    // ---- throughput: median of `reps` passes ----
    let run = |name: &str, f: &dyn Fn(&str) -> WordOutput| {
        let mut t = Vec::new();
        for _ in 0..reps {
            let s = Instant::now();
            for w in &words {
                std::hint::black_box(f(w));
            }
            t.push(s.elapsed().as_secs_f64());
        }
        t.sort_by(f64::total_cmp);
        println!("{name:<14} {:>9.0} words/s (median of {reps})", words.len() as f64 / t[reps / 2]);
    };
    run("fused strict", &|w| fused.lex(w, Mode::Strict));
    run("fused hybrid", &|w| fused.lex(w, Mode::Hybrid));
    Ok(())
}
