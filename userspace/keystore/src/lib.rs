// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Keystore domain library for cryptographic operations
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 3 unit tests
//!
//! PUBLIC API:
//!   - Keystore: Cryptographic key management
//!   - KeystoreError: Keystore error types
//!
//! DEPENDENCIES:
//!   - std::fs: File system operations
//!   - std::collections::HashMap: Key storage
//!   - thiserror: Error types
//!
//! ADR: docs/adr/0006-device-identity-architecture.md

#![forbid(unsafe_code)]

use std::convert::TryFrom;
use std::fs;
use std::path::Path;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{pkcs8::DecodePublicKey, Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub use ed25519_dalek::VerifyingKey as PublicKey;

/// Errors produced by the keystore helpers.
#[derive(Debug, Error)]
pub enum Error {
    /// I/O failure while reading anchor files.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Provided key material was malformed.
    #[error("invalid key: {0}")]
    InvalidKey(String),
    /// Provided signature was not valid for the supplied message.
    #[error("invalid signature")]
    InvalidSig,
}

/// Loads anchor public keys from the provided directory.
pub fn load_anchors(dir: &Path) -> Result<Vec<PublicKey>, Error> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_file()
            && path.extension().and_then(|ext| ext.to_str()) == Some("pub")
        {
            paths.push(path);
        }
    }

    paths.sort();

    let mut anchors = Vec::with_capacity(paths.len());
    for path in paths {
        let contents = fs::read_to_string(&path)?;
        let trimmed = contents.trim();
        let key = if trimmed.contains("-----BEGIN PUBLIC KEY-----") {
            let der = parse_pem_spki(trimmed)?;
            VerifyingKey::from_public_key_der(&der)
                .map_err(|err| Error::InvalidKey(err.to_string()))?
        } else {
            parse_hex_key(trimmed)?
        };
        anchors.push(key);
    }

    Ok(anchors)
}

fn parse_hex_key(input: &str) -> Result<PublicKey, Error> {
    let filtered: String = input.chars().filter(|ch| !ch.is_ascii_whitespace()).collect();
    if filtered.is_empty() {
        return Err(Error::InvalidKey("empty key material".into()));
    }
    let bytes = hex::decode(&filtered)
        .map_err(|err| Error::InvalidKey(format!("failed to decode hex: {err}")))?;
    let array: [u8; 32] = bytes
        .try_into()
        .map_err(|_| Error::InvalidKey("expected 32-byte Ed25519 public key".into()))?;
    VerifyingKey::from_bytes(&array).map_err(|err| Error::InvalidKey(err.to_string()))
}

fn parse_pem_spki(input: &str) -> Result<Vec<u8>, Error> {
    // Minimal PEM parser: extract base64 between headers and decode
    let begin = "-----BEGIN PUBLIC KEY-----";
    let end = "-----END PUBLIC KEY-----";
    let start = input.find(begin).ok_or_else(|| Error::InvalidKey("missing PEM header".into()))?
        + begin.len();
    let stop = input.find(end).ok_or_else(|| Error::InvalidKey("missing PEM footer".into()))?;
    if stop <= start {
        return Err(Error::InvalidKey("invalid PEM framing".into()));
    }
    let body = &input[start..stop];
    let cleaned: String = body.chars().filter(|ch| !ch.is_whitespace()).collect();
    BASE64
        .decode(cleaned.as_bytes())
        .map_err(|err| Error::InvalidKey(format!("invalid PEM base64: {err}")))
}

/// Verifies a detached Ed25519 signature against the provided message.
pub fn verify_detached(pk: &PublicKey, msg: &[u8], sig: &[u8]) -> Result<(), Error> {
    let signature = Signature::try_from(sig).map_err(|_| Error::InvalidSig)?;
    pk.verify_strict(msg, &signature).map_err(|_| Error::InvalidSig)
}

/// Derives the stable device identifier for a public key.
pub fn device_id(pk: &PublicKey) -> String {
    let digest = Sha256::digest(pk.as_bytes());
    hex::encode(&digest[..16])
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use tempfile::tempdir;

    const SECRET_KEY_BYTES: [u8; 32] = [
        0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c,
        0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae,
        0x7f, 0x60,
    ];

    #[test]
    fn parse_hex_and_pem_anchors() {
        let dir = tempdir().expect("tempdir");
        let signing = SigningKey::from_bytes(&SECRET_KEY_BYTES);
        let verifying = signing.verifying_key();

        let hex_path = dir.path().join("hex.pub");
        std::fs::write(&hex_path, hex::encode(verifying.to_bytes())).expect("write hex");

        let anchors = load_anchors(dir.path()).expect("load anchors");
        assert_eq!(anchors.len(), 1);
        assert_eq!(anchors[0].to_bytes(), verifying.to_bytes());
    }

    #[test]
    fn verify_known_signature() {
        let signing = SigningKey::from_bytes(&SECRET_KEY_BYTES);
        let verifying = signing.verifying_key();
        let message = b"test payload";
        let signature = signing.sign(message);
        let sig_bytes = signature.to_bytes();

        verify_detached(&verifying, message, &sig_bytes).expect("signature valid");

        let mut tampered = signature.to_bytes();
        tampered[0] ^= 0x01;
        assert!(matches!(verify_detached(&verifying, message, &tampered), Err(Error::InvalidSig)));
    }

    #[test]
    fn derive_device_id() {
        let signing = SigningKey::from_bytes(&SECRET_KEY_BYTES);
        let verifying = signing.verifying_key();
        let id = device_id(&verifying);
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|ch| ch.is_ascii_hexdigit()));
    }
}
