//! Write a benchmark corpus for a spec (same recipe as `bench`).
//! usage: corpus <spec.morph> <n_words>
use morph_fst::fused::derive_vocab;
use morph_fst::rng::Rng;
use morph_fst::{oracle, spec};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let (Some(path), Some(n)) = (a.get(1), a.get(2)) else {
        return Err("usage: corpus <spec.morph> <n_words>".into());
    };
    let text = std::fs::read_to_string(path)?;
    let n: usize = n.parse()?;
    let g = spec::load(&text)?;
    let good: Vec<String> = oracle::generate(&spec::parse_spec(&text)?, 4, 300_000)?.surfaces.into_keys().collect();
    let alpha: Vec<char> = derive_vocab(&g).iter().filter(|p| !p.cont && p.text.chars().count() == 1).filter_map(|p| p.text.chars().next()).filter(|c| c.is_alphabetic()).collect();
    let mut rng = Rng::new(7);
    let mut out = Vec::new();
    for i in 0..n {
        out.push(match i % 4 {
            0 | 1 => rng.pick(&good).clone(),
            2 => {
                let mut cs: Vec<char> = rng.pick(&good).chars().collect();
                let k = rng.below(cs.len());
                cs[k] = *rng.pick(&alpha);
                cs.into_iter().collect()
            }
            _ => (0..3 + rng.below(10)).map(|_| *rng.pick(&alpha)).collect::<String>(),
        });
    }
    println!("{}", out.join("\n"));
    Ok(())
}
