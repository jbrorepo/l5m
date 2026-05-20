use std::{fmt, io};

pub type Result<T> = std::result::Result<T, L5mError>;

#[derive(Debug)]
pub enum L5mError {
    Io(io::Error),
    Json(serde_json::Error),
    Format(String),
}

impl fmt::Display for L5mError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Json(err) => write!(f, "JSON error: {err}"),
            Self::Format(message) => write!(f, "segment format error: {message}"),
        }
    }
}

impl std::error::Error for L5mError {}

impl From<io::Error> for L5mError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for L5mError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}
