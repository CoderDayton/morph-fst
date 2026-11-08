//! Text in and results out: splitting running text into words, normalizing
//! them, and the [`Word`](crate::Word) view of what the engine found.

pub mod normalize;
pub mod output;
pub mod pretok;
pub(crate) mod search;
pub(crate) mod split;
pub(crate) mod tokenize;
pub(crate) mod word;
