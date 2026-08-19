//! The single error type used across the backend.
//!
//! Every `#[tauri::command]` returns `Result<T, String>` so the frontend gets a
//! sentence it can show verbatim. Internally the code works with [`AppError`],
//! which is converted at the command boundary via [`From<AppError> for String`].
//! Nothing is ever `Debug`-formatted into a user-facing message.

use std::fmt::Display;

/// Everything that can go wrong inside Ketikin, phrased for a human.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// A file could not be read or written in the resolved data directory.
    #[error("{0}")]
    Storage(String),

    /// A JSON payload could not be serialized.
    #[error("could not encode data as JSON: {0}")]
    Serialize(#[from] serde_json::Error),

    /// The caller referred to something that does not exist.
    #[error("{0}")]
    NotFound(String),

    /// The caller supplied a value we refuse to accept.
    #[error("{0}")]
    Invalid(String),

    /// The typing engine could not start, or failed mid-run.
    #[error("{0}")]
    Typing(String),

    /// A global shortcut could not be parsed or registered.
    #[error("{0}")]
    Hotkey(String),

    /// The update check, download, or install failed.
    #[error("{0}")]
    Updater(String),
}

impl From<AppError> for String {
    fn from(value: AppError) -> Self {
        value.to_string()
    }
}

impl AppError {
    /// Convenience for the many places that build a message from a source error.
    pub fn storage(context: impl Display, source: impl Display) -> Self {
        Self::Storage(format!("{context}: {source}"))
    }
}
