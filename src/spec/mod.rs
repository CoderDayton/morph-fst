//! Shared front end: the `.morph` spec language.
//!
//! * [`parser`] turns text into an [`ast::Spec`].
//! * [`expand`] validates it and compiles orthographic rules into
//!   allomorph entries with continuation *filters*, yielding a [`Grammar`].

pub mod ast;
pub mod expand;
pub mod parser;
pub mod zeros;

pub use ast::*;
pub use expand::{compile_grammar, ClassId, Entry, Filter, FilterKind, Grammar, Tracker, END};
pub use parser::parse_spec;

/// Parse + expand in one call.
pub fn load(text: &str) -> crate::Result<Grammar> {
    compile_grammar(&parse_spec(text)?)
}
