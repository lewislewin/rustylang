use thiserror::Error;

#[derive(Error, Debug)]
pub enum RustyLangError {
    #[error("Invalid dot path: {0}")]
    InvalidDotPath(String),
    #[error("Path not found: {0}")]
    PathNotFound(String),
}



