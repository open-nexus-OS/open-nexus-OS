// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Host-first manifest parser for the bundle manager service.
//!
//! The parser intentionally focuses on validation and reporting rather than I/O
//! so it can be exercised directly from tests. It reads a TOML document and
//! enforces a minimal schema containing name, version, declared abilities,
//! capabilities, and the minimum supported SDK version.

#![deny(clippy::all, missing_docs)]

use semver::Version;
use thiserror::Error;
use toml::{self, Value};

const KNOWN_KEYS: &[&str] = &["name", "version", "abilities", "caps", "min_sdk"];

/// Result alias returned by the parser.
pub type Result<T> = core::result::Result<T, Error>;

/// Errors emitted while parsing bundle manifests.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    /// The manifest failed to parse as TOML.
    #[error("manifest parse error: {0}")]
    Toml(String),
    /// A required field was not provided.
    #[error("missing required field `{0}`")]
    MissingField(&'static str),
    /// The root element is not a TOML table.
    #[error("manifest root must be a TOML table")]
    InvalidRoot,
    /// A field contained a malformed value.
    #[error("invalid field `{field}`: {reason}")]
    InvalidField {
        /// Name of the offending field.
        field: &'static str,
        /// Human-readable reason for the failure.
        reason: String,
    },
}

/// Parsed manifest contents used by bundle manager host tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Bundle identifier.
    pub name: String,
    /// Human readable bundle version.
    pub version: Version,
    /// Declared abilities exposed by the bundle.
    pub abilities: Vec<String>,
    /// Capability requirements requested by the bundle.
    pub capabilities: Vec<String>,
    /// Minimum SDK version compatible with the bundle.
    pub min_sdk: Version,
    /// Non-fatal warnings produced during parsing.
    pub warnings: Vec<String>,
}

impl Manifest {
    /// Parses a manifest from a UTF-8 TOML string.
    pub fn parse_str(input: &str) -> Result<Self> {
        let value: Value = toml::from_str(input).map_err(|err| Error::Toml(err.to_string()))?;
        let table: &toml::Table = value.as_table().ok_or(Error::InvalidRoot)?;

        let mut warnings = Vec::new();
        for key in table.keys() {
            if !KNOWN_KEYS.contains(&key.as_str()) {
                warnings.push(format!("unknown key `{key}`"));
            }
        }

        let name_raw = require_string(table, "name")?;
        let name_trimmed = name_raw.trim();
        if name_trimmed.is_empty() {
            return Err(Error::InvalidField { field: "name", reason: "must not be empty".into() });
        }
        let name = name_trimmed.to_string();

        let version = parse_version(table, "version")?;
        let abilities = require_string_array(table, "abilities")?;
        if abilities.is_empty() {
            return Err(Error::InvalidField {
                field: "abilities",
                reason: "must contain at least one entry".into(),
            });
        }

        let capabilities = require_string_array(table, "caps")?;
        if capabilities.iter().any(|cap| cap.trim().is_empty()) {
            return Err(Error::InvalidField {
                field: "caps",
                reason: "entries must not be empty".into(),
            });
        }

        let min_sdk = parse_version(table, "min_sdk")?;

        Ok(Self { name, version, abilities, capabilities, min_sdk, warnings })
    }
}

fn parse_version(table: &toml::Table, field: &'static str) -> Result<Version> {
    let raw = require_string(table, field)?;
    Version::parse(raw.trim()).map_err(|err| Error::InvalidField { field, reason: err.to_string() })
}

fn require_string(table: &toml::Table, field: &'static str) -> Result<String> {
    match table.get(field) {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(_) => Err(Error::InvalidField { field, reason: "expected string".into() }),
        None => Err(Error::MissingField(field)),
    }
}

fn require_string_array(table: &toml::Table, field: &'static str) -> Result<Vec<String>> {
    let raw = table.get(field).ok_or(Error::MissingField(field))?;
    let array = raw
        .as_array()
        .ok_or_else(|| Error::InvalidField { field, reason: "expected array of strings".into() })?;

    let mut values = Vec::with_capacity(array.len());
    for item in array {
        let value = item
            .as_str()
            .ok_or_else(|| Error::InvalidField {
                field,
                reason: "expected array of strings".into(),
            })?
            .trim();
        if value.is_empty() {
            return Err(Error::InvalidField { field, reason: "entries must not be empty".into() });
        }
        values.push(value.to_string());
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_name() {
        let manifest = r#"
            name = ""
            version = "1.0.0"
            abilities = ["svc"]
            caps = ["camera"]
            min_sdk = "0.1.0"
        "#;
        let err = Manifest::parse_str(manifest).unwrap_err();
        assert!(matches!(err, Error::InvalidField { field: "name", .. }));
    }
}
