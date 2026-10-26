//! LinMaxMatch (O(n), failure links) must equal naive greedy MaxMatch,
//! including the byte-fallback rule, on random vocabularies and inputs.

use morph_fst::rng::Rng;
use morph_fst::subword::reference::ReferenceMaxMatch;
use morph_fst::subword::{build_model, closed_piece_set, PieceDef};

const VOCAB_ALPHA: &[char] = &['a', 'b', 'c', '#', 'é'];
const INPUT_EXTRA: &[char] = &['x', '😀', '漢', '#'];

fn rand_str(rng: &mut Rng, alpha: &[char], max: usize) -> String {
    (0..1 + rng.below(max)).map(|_| *rng.pick(alpha)).collect()
}

#[test]
fn linmax_equals_reference_on_random_vocabs() {
    let mut rng = Rng::new(42);
    let input_alpha: Vec<char> = VOCAB_ALPHA.iter().chain(INPUT_EXTRA).copied().collect();
    for round in 0..1500 {
        let n_pieces = 1 + rng.below(25);
        let pieces: Vec<PieceDef> = (0..n_pieces)
            .map(|_| PieceDef { text: rand_str(&mut rng, VOCAB_ALPHA, 5), cont: rng.chance(1, 2) })
            .collect();
        let model = build_model(&pieces);
        let closed = closed_piece_set(pieces.iter().map(|p| (p.text.as_str(), p.cont)));
        let reference = ReferenceMaxMatch::new(closed.iter().map(|(c, t)| (t.as_str(), *c)));
        for _ in 0..60 {
            let w = rand_str(&mut rng, &input_alpha, 24);
            let mut steps = 0;
            let fast = model.tokenize_counted(&w, &mut steps);
            assert_eq!(fast, reference.tokenize(&w), "round {round} word {w:?} pieces {pieces:?}");
            assert!(steps <= 2 * w.chars().count() as u64, "steps {steps} for {w:?}");
            assert_eq!(morph_fst::output::reconstruct(&fast), w.as_bytes());
        }
        assert!(model.tokenize("").is_empty());
    }
}

#[test]
fn long_adversarial_input_is_linear() {
    // Classic MaxMatch worst case: vocab {a, aa..a(k), ##a..} and "aaaa...ab".
    let mut pieces: Vec<PieceDef> = (1..=40).map(|k| PieceDef { text: "a".repeat(k), cont: false }).collect();
    pieces.extend((1..=40).map(|k| PieceDef { text: "a".repeat(k), cont: true }));
    pieces.push(PieceDef { text: "a".repeat(39) + "b", cont: true });
    let model = build_model(&pieces);
    let w = "a".repeat(200_000) + "c";
    let mut steps = 0;
    let out = model.tokenize_counted(&w, &mut steps);
    assert!(steps <= 2 * w.len() as u64);
    assert_eq!(morph_fst::output::reconstruct(&out), w.as_bytes());
}
