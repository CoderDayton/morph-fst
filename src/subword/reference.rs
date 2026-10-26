//! Naive greedy longest-match-first WordPiece (O(n·m)) with the *same*
//! per-character byte-fallback rule as LinMaxMatch. Test oracle only.

use std::collections::HashSet;

use crate::output::SubToken;

pub struct ReferenceMaxMatch {
    init: HashSet<String>,
    cont: HashSet<String>,
    max_chars: usize,
}

impl ReferenceMaxMatch {
    pub fn new<'a, I: IntoIterator<Item = (&'a str, bool)>>(closed_pieces: I) -> Self {
        let mut r = ReferenceMaxMatch { init: HashSet::new(), cont: HashSet::new(), max_chars: 0 };
        for (t, c) in closed_pieces {
            r.max_chars = r.max_chars.max(t.chars().count());
            if c {
                r.cont.insert(t.to_string());
            } else {
                r.init.insert(t.to_string());
            }
        }
        r
    }

    pub fn tokenize(&self, word: &str) -> Vec<SubToken> {
        let bounds: Vec<usize> = word.char_indices().map(|(i, _)| i).chain(std::iter::once(word.len())).collect();
        let mut out = Vec::new();
        let mut i = 0; // index into bounds
        let mut first = true;
        while i + 1 < bounds.len() {
            let set = if first { &self.init } else { &self.cont };
            let maxj = (i + self.max_chars).min(bounds.len() - 1);
            let mut hit = None;
            for j in (i + 1..=maxj).rev() {
                if set.contains(&word[bounds[i]..bounds[j]]) {
                    hit = Some(j);
                    break;
                }
            }
            match hit {
                Some(j) => {
                    out.push(SubToken::Piece { text: word[bounds[i]..bounds[j]].to_string(), cont: !first });
                    i = j;
                }
                None => {
                    out.extend(word[bounds[i]..bounds[i + 1]].bytes().map(SubToken::Byte));
                    i += 1;
                }
            }
            first = false;
        }
        out
    }
}
