use std::process::ExitCode;

use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("invalid configuration: {0}")]
    Config(String),
    #[error("permission denied: {0}")]
    Denied(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("unavailable: {0}")]
    Unavailable(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl Error {
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::InvalidInput(_) | Self::Config(_) => ExitCode::from(2),
            Self::Denied(_) => ExitCode::from(77),
            Self::NotFound(_) => ExitCode::from(127),
            Self::Unavailable(_) => ExitCode::from(69),
            Self::Io(_) | Self::Sqlite(_) | Self::Json(_) => ExitCode::from(70),
        }
    }
}
