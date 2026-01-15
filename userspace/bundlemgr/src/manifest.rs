// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// CONTEXT: Canonical bundle manifest parsing for bundle manager
// OWNERS: @runtime
// STATUS: Functional
// API_STABILITY: Stable (v1.0)
// TEST_COVERAGE: 6 manifest tests
//   - manifest.nxb (Cap'n Proto) decode + validation
//   - reject invalid/malformed manifests (bounded)
//   - signature/publisher byte length validation
//
// NOTE:
//   `manifest.nxb` is the single on-disk source of truth for `.nxb` bundles.
//   Human-editable TOML is tooling input only (compiled by `nxb-pack`).
//   See ADR-0020: docs/adr/0020-manifest-format-capnproto.md

#![deny(clippy::all, missing_docs)]

use std::io::Cursor;

use capnp::message::ReaderOptions;
use capnp::serialize;
use semver::Version;
use thiserror::Error;

use nexus_idl_runtime::manifest_capnp::bundle_manifest;

/// Result alias returned by the parser.
pub type Result<T> = core::result::Result<T, Error>;

/// Errors emitted while parsing bundle manifests.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    /// The manifest failed to decode as Cap'n Proto.
    #[error("manifest decode error: {0}")]
    Decode(String),
    /// A required field was not provided.
    #[error("missing required field `{0}`")]
    MissingField(&'static str),
    /// A field contained a malformed value.
    #[error("invalid field `{field}`: {reason}")]
    InvalidField {
        /// Name of the offending field.
        field: &'static str,
        /// Human-readable reason for the failure.
        reason: String,
    },
}

/// Parsed manifest contents used by bundle manager.
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
    /// Anchor identifier of the publisher (lowercase hex, 64 chars).
    pub publisher: String,
    /// Detached Ed25519 signature covering the bundle payload (64 bytes).
    pub signature: Vec<u8>,
    /// Non-fatal warnings produced during parsing.
    pub warnings: Vec<String>,
}

impl Manifest {
    /// Parses a canonical `manifest.nxb` from Cap'n Proto bytes.
    pub fn parse_nxb(bytes: &[u8]) -> Result<Self> {
        let mut cursor = Cursor::new(bytes);
        let message = serialize::read_message(&mut cursor, ReaderOptions::new())
            .map_err(|err| Error::Decode(err.to_string()))?;
        let m = message
            .get_root::<bundle_manifest::Reader<'_>>()
            .map_err(|err| Error::Decode(err.to_string()))?;

        let name_raw = m.get_name().map_err(|err| Error::Decode(err.to_string()))?;
        let name_raw = name_raw.to_str().map_err(|err| Error::InvalidField {
            field: "name",
            reason: format!("invalid utf-8: {err}"),
        })?;
        let name_trimmed = name_raw.trim();
        if name_trimmed.is_empty() {
            return Err(Error::InvalidField { field: "name", reason: "must not be empty".into() });
        }
        let name = name_trimmed.to_string();

        let semver_raw = m.get_semver().map_err(|err| Error::Decode(err.to_string()))?;
        let semver_raw = semver_raw.to_str().map_err(|err| Error::InvalidField {
            field: "semver",
            reason: format!("invalid utf-8: {err}"),
        })?;
        let version = Version::parse(semver_raw.trim())
            .map_err(|err| Error::InvalidField { field: "semver", reason: err.to_string() })?;

        let min_sdk_raw = m.get_min_sdk().map_err(|err| Error::Decode(err.to_string()))?;
        let min_sdk_raw = min_sdk_raw.to_str().map_err(|err| Error::InvalidField {
            field: "minSdk",
            reason: format!("invalid utf-8: {err}"),
        })?;
        let min_sdk = Version::parse(min_sdk_raw.trim())
            .map_err(|err| Error::InvalidField { field: "minSdk", reason: err.to_string() })?;

        let abilities_list = m.get_abilities().map_err(|err| Error::Decode(err.to_string()))?;
        if abilities_list.is_empty() {
            return Err(Error::InvalidField {
                field: "abilities",
                reason: "must contain at least one entry".into(),
            });
        }
        let mut abilities = Vec::with_capacity(abilities_list.len() as usize);
        for i in 0..abilities_list.len() {
            let s = abilities_list.get(i).map_err(|err| Error::Decode(err.to_string()))?;
            let s = s.to_str().map_err(|err| Error::InvalidField {
                field: "abilities",
                reason: format!("invalid utf-8: {err}"),
            })?;
            let t = s.trim();
            if t.is_empty() {
                return Err(Error::InvalidField {
                    field: "abilities",
                    reason: "entries must not be empty".into(),
                });
            }
            abilities.push(t.to_string());
        }

        let caps_list = m.get_capabilities().map_err(|err| Error::Decode(err.to_string()))?;
        let mut capabilities = Vec::with_capacity(caps_list.len() as usize);
        for i in 0..caps_list.len() {
            let s = caps_list.get(i).map_err(|err| Error::Decode(err.to_string()))?;
            let s = s.to_str().map_err(|err| Error::InvalidField {
                field: "caps",
                reason: format!("invalid utf-8: {err}"),
            })?;
            let t = s.trim();
            if t.is_empty() {
                return Err(Error::InvalidField {
                    field: "caps",
                    reason: "entries must not be empty".into(),
                });
            }
            capabilities.push(t.to_string());
        }

        let publisher = m.get_publisher().map_err(|err| Error::Decode(err.to_string()))?;
        if publisher.len() != 16 {
            return Err(Error::InvalidField {
                field: "publisher",
                reason: format!("must be 16 bytes, got {}", publisher.len()),
            });
        }
        let publisher_hex = hex::encode(publisher);

        let signature = m.get_signature().map_err(|err| Error::Decode(err.to_string()))?;
        if signature.len() != 64 {
            return Err(Error::InvalidField {
                field: "signature",
                reason: format!("must be 64 bytes, got {}", signature.len()),
            });
        }

        Ok(Self {
            name,
            version,
            abilities,
            capabilities,
            min_sdk,
            publisher: publisher_hex,
            signature: signature.to_vec(),
            warnings: Vec::new(),
        })
    }
}
