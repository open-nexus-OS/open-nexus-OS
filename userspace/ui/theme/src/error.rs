// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

/// Result type alias for theme operations.
pub type ThemeResult<T> = Result<T, ThemeError>;

/// Errors that can occur during theme loading and resolution.
#[derive(Debug, thiserror::Error)]
pub enum ThemeError {
    #[error("IO error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("TOML parse error at {path}: {message}")]
    Parse { path: PathBuf, message: String },

    #[error("schema validation error at {path}: {message}")]
    SchemaValidation { path: PathBuf, message: String },

    #[error("invalid color value '{value}': {reason}")]
    InvalidColor { value: String, reason: String },

    #[error("missing base theme in directory: {dir}")]
    MissingBaseTheme { dir: PathBuf },

    #[error("token '{token}' not found (active qualifier: {qualifier:?})")]
    TokenNotFound { token: String, qualifier: crate::qualifier::Qualifier },

    #[error("unknown theme key '{key}' at {path}")]
    UnknownKey { key: String, path: PathBuf },

    #[error("unknown TOML section '[{section}]' at {path}")]
    UnknownSection { section: String, path: PathBuf },

    #[error("missing required section '[{section}]' at {path}")]
    MissingSection { section: String, path: PathBuf },
}
