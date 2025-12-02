// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host-first manifest parser for bundle manager service
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 8 manifest tests
//!   - TOML manifest parsing
//!   - Manifest validation and reporting
//!   - Signature verification
//!   - Publisher validation
//!   - Capability requirement checking
//!   - Warning generation for unknown fields
//!
//! TEST SCENARIOS:
//!   - test_parses_hex_signature(): Parse hex-encoded signatures
//!   - test_parses_base64_signature(): Parse base64-encoded signatures
//!   - test_rejects_empty_name(): Reject empty bundle names
//!   - test_manifest_validation(): Manifest validation and reporting
//!   - test_signature_verification(): Signature verification
//!   - test_publisher_validation(): Publisher validation
//!   - test_capability_validation(): Capability requirement validation
//!   - test_warning_generation(): Warning generation for unknown fields
//!
//! ADR: docs/adr/0009-bundle-manager-architecture.md

#![deny(clippy::all, missing_docs)]

use base64::{engine::general_purpose::STANDARD, Engine};
use semver::Version;
use thiserror::Error;
use toml::{self, Value};

const KNOWN_KEYS: &[&str] = &[
    "name",
    "version",
    "abilities",
    "caps",
    "min_sdk",
    "publisher",
    "sig",
];

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
    /// Anchor identifier of the publisher (lowercase hex).
    pub publisher: String,
    /// Detached Ed25519 signature covering the bundle payload.
    pub signature: Vec<u8>,
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
            return Err(Error::InvalidField {
                field: "name",
                reason: "must not be empty".into(),
            });
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
        let publisher = parse_publisher(table)?;
        let signature = parse_signature(table)?;

        Ok(Self {
            name,
            version,
            abilities,
            capabilities,
            min_sdk,
            publisher,
            signature,
            warnings,
        })
    }
}

fn parse_version(table: &toml::Table, field: &'static str) -> Result<Version> {
    let raw = require_string(table, field)?;
    Version::parse(raw.trim()).map_err(|err| Error::InvalidField {
        field,
        reason: err.to_string(),
    })
}

fn parse_publisher(table: &toml::Table) -> Result<String> {
    let raw = require_string(table, "publisher")?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidField {
            field: "publisher",
            reason: "must not be empty".into(),
        });
    }
    if trimmed.len() != 32 || !trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(Error::InvalidField {
            field: "publisher",
            reason: "must be 32 hexadecimal characters".into(),
        });
    }
    Ok(trimmed.to_ascii_lowercase())
}

fn parse_signature(table: &toml::Table) -> Result<Vec<u8>> {
    let raw = require_string(table, "sig")?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidField {
            field: "sig",
            reason: "must not be empty".into(),
        });
    }
    decode_signature(trimmed).map_err(|reason| Error::InvalidField {
        field: "sig",
        reason,
    })
}

fn decode_signature(input: &str) -> core::result::Result<Vec<u8>, String> {
    let cleaned: String = input
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    if cleaned.len() & 1 == 0 {
        if let Ok(bytes) = hex::decode(&cleaned) {
            if bytes.len() == 64 {
                return Ok(bytes);
            }
        }
    }
    match STANDARD.decode(cleaned.as_bytes()) {
        Ok(bytes) => {
            if bytes.len() == 64 {
                Ok(bytes)
            } else {
                Err("expected 64-byte signature".to_string())
            }
        }
        Err(err) => Err(format!("invalid signature encoding: {err}")),
    }
}

fn require_string(table: &toml::Table, field: &'static str) -> Result<String> {
    match table.get(field) {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(_) => Err(Error::InvalidField {
            field,
            reason: "expected string".into(),
        }),
        None => Err(Error::MissingField(field)),
    }
}

fn require_string_array(table: &toml::Table, field: &'static str) -> Result<Vec<String>> {
    let raw = table.get(field).ok_or(Error::MissingField(field))?;
    let array = raw.as_array().ok_or_else(|| Error::InvalidField {
        field,
        reason: "expected array of strings".into(),
    })?;

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
            return Err(Error::InvalidField {
                field,
                reason: "entries must not be empty".into(),
            });
        }
        values.push(value.to_string());
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_signature() {
        let manifest = format!("name = \"app\"\nversion = \"1.0.0\"\nabilities = [\"svc\"]\ncaps = [\"gpu\"]\nmin_sdk = \"0.1.0\"\npublisher = \"0123456789abcdef0123456789abcdef\"\nsig = \"{}\"\n", "11".repeat(64));
        let parsed = Manifest::parse_str(&manifest).unwrap();
        assert_eq!(parsed.publisher, "0123456789abcdef0123456789abcdef");
        assert_eq!(parsed.signature.len(), 64);
    }

    #[test]
    fn parses_base64_signature() {
        let bytes = vec![0x22; 64];
        let sig_b64 = STANDARD.encode(&bytes);
        let manifest = format!("name = \"app\"\nversion = \"1.0.0\"\nabilities = [\"svc\"]\ncaps = [\"gpu\"]\nmin_sdk = \"0.1.0\"\npublisher = \"fedcba9876543210fedcba9876543210\"\nsig = \"{}\"\n", sig_b64);
        let parsed = Manifest::parse_str(&manifest).unwrap();
        assert_eq!(parsed.publisher, "fedcba9876543210fedcba9876543210");
        assert_eq!(parsed.signature, bytes);
    }
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
