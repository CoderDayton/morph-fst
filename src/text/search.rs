//! [`Index`]: lexical scoring of documents against a query, over lemmas.

use std::collections::HashMap;

use crate::Lexer;

/// BM25 term-frequency saturation and length normalization, at the
/// published defaults (Robertson & Zaragoza, 2009). They are the standard
/// values, not tuned on any data here.
const K1: f64 = 1.2;
const B: f64 = 0.75;

/// Documents scored against queries by their lemmas, with BM25: a query
/// and a document meet however their words are inflected (`ran` finds
/// `running`), rare shared words count more than common ones, and long
/// documents do not win by length alone. Build one with
/// [`Lexer::index`].
///
/// ```
/// use morph_fst::Lexer;
///
/// let lx = Lexer::bundled("english")?;
/// let index = lx.index(["The cat ran home.", "Dogs are running in the park.", "A quiet morning."]);
/// let hits = index.search("dog runs");
/// assert_eq!(hits[0].0, 1);
/// assert_eq!(hits.len(), 2);
/// # Ok::<(), morph_fst::Error>(())
/// ```
#[derive(Clone, Debug)]
pub struct Index<'a> {
    lexer: &'a Lexer,
    /// Per document: lemma → count.
    docs: Vec<HashMap<String, u32>>,
    lengths: Vec<u32>,
    /// Lemma → number of documents it occurs in.
    df: HashMap<String, u32>,
    total_len: u64,
}

impl Lexer {
    /// An [`Index`] over `docs` (document ids are their positions), for
    /// lexical search. Add more later with [`Index::add`].
    pub fn index<'d>(&self, docs: impl IntoIterator<Item = &'d str>) -> Index<'_> {
        let mut index = Index { lexer: self, docs: Vec::new(), lengths: Vec::new(), df: HashMap::new(), total_len: 0 };
        for d in docs {
            index.add(d);
        }
        index
    }
}

impl Index<'_> {
    /// Add a document; returns its id.
    pub fn add(&mut self, doc: &str) -> usize {
        let mut counts: HashMap<String, u32> = HashMap::new();
        let terms = self.lexer.terms(doc);
        for t in &terms {
            *counts.entry(t.clone()).or_default() += 1;
        }
        for t in counts.keys() {
            *self.df.entry(t.clone()).or_default() += 1;
        }
        let len = u32::try_from(terms.len()).unwrap_or(u32::MAX);
        self.total_len = self.total_len.saturating_add(u64::from(len));
        self.lengths.push(len);
        self.docs.push(counts);
        self.docs.len() - 1
    }

    /// Number of documents.
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    /// Whether the index has no documents.
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// The score of document `doc` for `query` (0 when they share no
    /// lemma, or `doc` is not in the index).
    pub fn score(&self, query: &str, doc: usize) -> f64 {
        let terms = self.query_terms(query);
        self.score_terms(&terms, doc)
    }

    /// Documents that share a lemma with `query`, best first, as
    /// `(document id, score)`. Equal scores keep document order.
    pub fn search(&self, query: &str) -> Vec<(usize, f64)> {
        let terms = self.query_terms(query);
        let mut hits: Vec<(usize, f64)> = (0..self.docs.len()).map(|d| (d, self.score_terms(&terms, d))).filter(|(_, s)| *s > 0.0).collect();
        hits.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        hits
    }

    /// Distinct query lemmas: a repeated query word counts once.
    fn query_terms(&self, query: &str) -> Vec<String> {
        let mut terms = self.lexer.terms(query);
        terms.sort_unstable();
        terms.dedup();
        terms
    }

    fn score_terms(&self, terms: &[String], doc: usize) -> f64 {
        let (Some(counts), Some(&len)) = (self.docs.get(doc), self.lengths.get(doc)) else { return 0.0 };
        let n = self.docs.len() as f64;
        let avg = if self.docs.is_empty() { 1.0 } else { (self.total_len as f64 / n).max(1.0) };
        terms
            .iter()
            .filter_map(|t| {
                let tf = f64::from(*counts.get(t)?);
                let df = f64::from(self.df.get(t).copied().unwrap_or(0));
                let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
                Some(idf * tf * (K1 + 1.0) / (tf + K1 * (1.0 - B + B * f64::from(len) / avg)))
            })
            .sum()
    }
}
