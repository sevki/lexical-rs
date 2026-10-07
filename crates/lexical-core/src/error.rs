use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    NoSelection,
    MissingNode(String),
    InvalidJson(String),
    TransformLoop,
    Invalid(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NoSelection => write!(f, "no selection"),
            Error::MissingNode(k) => write!(f, "node {k} does not exist"),
            Error::InvalidJson(m) => write!(f, "invalid editor state json: {m}"),
            Error::TransformLoop => write!(f, "node transforms did not settle"),
            Error::Invalid(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
