//! Crate-wide error type. Nothing in this crate panics on bad *input*
//! (specs, vocab files, artifacts, text); every failure surfaces here.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// A problem in a `.morph` spec, with its 1-based line number (0 = global).
    Spec { line: usize, msg: String },
    /// A corrupt, truncated or mismatched binary artifact.
    Artifact(String),
    /// Filesystem trouble.
    Io(String),
    /// [`crate::Lexer::bundled`] was given a name not in [`crate::languages`].
    UnknownLanguage(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Spec { line: 0, msg } => write!(f, "spec error: {msg}"),
            Error::Spec { line, msg } => write!(f, "spec error (line {line}): {msg}"),
            Error::Artifact(m) => write!(f, "artifact error: {m}"),
            Error::Io(m) => write!(f, "io error: {m}"),
            Error::UnknownLanguage(n) => {
                write!(f, "no bundled language named `{n}` (try one of: {})", crate::languages::NAMES.join(", "))
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn spec_err<T>(line: usize, msg: impl Into<String>) -> Result<T> {
    Err(Error::Spec { line, msg: msg.into() })
}

pub(crate) fn art_err<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::Artifact(msg.into()))
}
