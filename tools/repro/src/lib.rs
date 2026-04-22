//! CONTEXT: Repro metadata capture and verification for `.nxb` bundles
//! OWNERS: @runtime @security
//! STATUS: Functional
//! API_STABILITY: Unstable (internal tooling contract)
//! TEST_COVERAGE: 4 unit tests
//!   - verify_accepts_valid_schema_and_digests
//!   - verify_reject_repro_schema_invalid_extra_field
//!   - verify_rejects_digest_mismatch
//!   - capture_rejects_oversized_rustflags
//!
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md
#![forbid(unsafe_code)]

use nexus_evidence::{scan_for_secrets_with, test_support, EvidenceError, ScanAllowlist};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

const MAX_SBOM_BYTES: usize = 512 * 1024;
const MAX_REPRO_JSON_BYTES: usize = 256 * 1024;
const MAX_RUSTC_LEN: usize = 64;
const MAX_TARGET_LEN: usize = 64;
const MAX_RUSTFLAGS_LEN: usize = 262_144;

#[derive(Debug, Error)]
pub enum ReproError {
    #[error("invalid SOURCE_DATE_EPOCH: {0}")]
    InvalidSourceDateEpoch(String),
    #[error("invalid digest hex for `{field}`")]
    InvalidDigestHex { field: &'static str },
    #[error("repro schema invalid: {0}")]
    SchemaInvalid(String),
    #[error("repro digest mismatch: {field}")]
    DigestMismatch { field: &'static str },
    #[error("repro secret scan failed: pattern={pattern} artifact={artifact} line={line}")]
    SecretLeak { pattern: &'static str, artifact: &'static str, line: usize },
    #[error("repro secret scan failed: {0}")]
    SecretScanner(String),
    #[error("repro serialization failed: {0}")]
    Serialize(String),
    #[error("input too large: {field} (max={max}, actual={actual})")]
    InputTooLarge { field: &'static str, max: usize, actual: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReproVerifyInput {
    pub payload_sha256: String,
    pub manifest_sha256: String,
    pub sbom_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReproEnvV1 {
    pub schema_version: u8,
    pub source_date_epoch: u64,
    pub payload_sha256: String,
    pub manifest_sha256: String,
    pub sbom_sha256: String,
    pub rustc: String,
    pub target: String,
    pub rustflags: String,
}

pub fn source_date_epoch_from_env() -> Result<u64, ReproError> {
    match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(raw) => raw.parse::<u64>().map_err(|_| ReproError::InvalidSourceDateEpoch(raw)),
        Err(std::env::VarError::NotPresent) => Ok(0),
        Err(err) => Err(ReproError::InvalidSourceDateEpoch(err.to_string())),
    }
}

pub fn capture_bundle_repro_json(
    manifest_bytes: &[u8],
    payload_bytes: &[u8],
    sbom_bytes: &[u8],
) -> Result<Vec<u8>, ReproError> {
    let manifest_sha256 = sha256_hex(manifest_bytes);
    capture_bundle_repro_json_with_manifest_digest(&manifest_sha256, payload_bytes, sbom_bytes)
}

pub fn capture_bundle_repro_json_with_manifest_digest(
    manifest_sha256: &str,
    payload_bytes: &[u8],
    sbom_bytes: &[u8],
) -> Result<Vec<u8>, ReproError> {
    if sbom_bytes.len() > MAX_SBOM_BYTES {
        return Err(ReproError::InputTooLarge {
            field: "sbom_bytes",
            max: MAX_SBOM_BYTES,
            actual: sbom_bytes.len(),
        });
    }
    validate_digest(manifest_sha256, "manifest_sha256")?;
    let rustc = std::env::var("NEXUS_REPRO_RUSTC").unwrap_or_else(|_| "unknown".to_string());
    if rustc.len() > MAX_RUSTC_LEN {
        return Err(ReproError::InputTooLarge {
            field: "rustc",
            max: MAX_RUSTC_LEN,
            actual: rustc.len(),
        });
    }
    let target = std::env::var("NEXUS_REPRO_TARGET").unwrap_or_else(|_| "unknown".to_string());
    if target.len() > MAX_TARGET_LEN {
        return Err(ReproError::InputTooLarge {
            field: "target",
            max: MAX_TARGET_LEN,
            actual: target.len(),
        });
    }
    let rustflags = std::env::var("RUSTFLAGS").unwrap_or_default();
    if rustflags.len() > MAX_RUSTFLAGS_LEN {
        return Err(ReproError::InputTooLarge {
            field: "rustflags",
            max: MAX_RUSTFLAGS_LEN,
            actual: rustflags.len(),
        });
    }
    let record = ReproEnvV1 {
        schema_version: 1,
        source_date_epoch: source_date_epoch_from_env()?,
        payload_sha256: sha256_hex(payload_bytes),
        manifest_sha256: manifest_sha256.to_string(),
        sbom_sha256: sha256_hex(sbom_bytes),
        rustc,
        target,
        rustflags,
    };
    let bytes =
        serde_json::to_vec(&record).map_err(|err| ReproError::Serialize(err.to_string()))?;
    if bytes.len() > MAX_REPRO_JSON_BYTES {
        return Err(ReproError::InputTooLarge {
            field: "repro_json",
            max: MAX_REPRO_JSON_BYTES,
            actual: bytes.len(),
        });
    }
    let allowlist = digest_allowlist(&record)?;
    reject_if_secret_leak(&bytes, &allowlist)?;
    Ok(bytes)
}

pub fn verify_repro_json(
    bytes: &[u8],
    expected: &ReproVerifyInput,
) -> Result<ReproEnvV1, ReproError> {
    if bytes.len() > MAX_REPRO_JSON_BYTES {
        return Err(ReproError::InputTooLarge {
            field: "repro_json",
            max: MAX_REPRO_JSON_BYTES,
            actual: bytes.len(),
        });
    }
    let parsed: ReproEnvV1 =
        serde_json::from_slice(bytes).map_err(|err| ReproError::SchemaInvalid(err.to_string()))?;
    if parsed.schema_version != 1 {
        return Err(ReproError::SchemaInvalid("schema_version must be 1".to_string()));
    }
    validate_digest(&parsed.payload_sha256, "payload_sha256")?;
    validate_digest(&parsed.manifest_sha256, "manifest_sha256")?;
    validate_digest(&parsed.sbom_sha256, "sbom_sha256")?;
    validate_digest(&expected.payload_sha256, "expected.payload_sha256")?;
    validate_digest(&expected.manifest_sha256, "expected.manifest_sha256")?;
    validate_digest(&expected.sbom_sha256, "expected.sbom_sha256")?;

    if parsed.payload_sha256 != expected.payload_sha256 {
        return Err(ReproError::DigestMismatch { field: "payload_sha256" });
    }
    if parsed.manifest_sha256 != expected.manifest_sha256 {
        return Err(ReproError::DigestMismatch { field: "manifest_sha256" });
    }
    if parsed.sbom_sha256 != expected.sbom_sha256 {
        return Err(ReproError::DigestMismatch { field: "sbom_sha256" });
    }
    Ok(parsed)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn validate_digest(value: &str, field: &'static str) -> Result<(), ReproError> {
    if value.len() != 64 {
        return Err(ReproError::InvalidDigestHex { field });
    }
    let decoded = hex::decode(value).map_err(|_| ReproError::InvalidDigestHex { field })?;
    if decoded.len() != 32 {
        return Err(ReproError::InvalidDigestHex { field });
    }
    Ok(())
}

fn digest_allowlist(record: &ReproEnvV1) -> Result<ScanAllowlist, ReproError> {
    let mut fragments = vec![
        record.payload_sha256.clone(),
        record.manifest_sha256.clone(),
        record.sbom_sha256.clone(),
    ];
    if !record.rustflags.is_empty() {
        fragments.push(record.rustflags.clone());
    }
    let escaped = fragments
        .into_iter()
        .map(|fragment| fragment.replace('\\', "\\\\").replace('"', "\\\""))
        .collect::<Vec<_>>()
        .join("\",\"");
    let toml = format!("[allowlist]\nsubstrings=[\"{escaped}\"]\n");
    ScanAllowlist::from_toml(&toml).map_err(|err| ReproError::SecretScanner(err.to_string()))
}

fn reject_if_secret_leak(bytes: &[u8], allowlist: &ScanAllowlist) -> Result<(), ReproError> {
    let mut bundle = test_support::empty_bundle();
    bundle.uart.bytes = bytes.to_vec();
    scan_for_secrets_with(&bundle, allowlist).map_err(|err| match err {
        EvidenceError::SecretLeak { artifact, line, pattern } => {
            ReproError::SecretLeak { artifact, line, pattern }
        }
        other => ReproError::SecretScanner(other.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct RustflagsRestore(Option<String>);

    impl Drop for RustflagsRestore {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => std::env::set_var("RUSTFLAGS", value),
                None => std::env::remove_var("RUSTFLAGS"),
            }
        }
    }

    fn with_rustflags<T>(value: String, f: impl FnOnce() -> T) -> T {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let restore = RustflagsRestore(std::env::var("RUSTFLAGS").ok());
        std::env::set_var("RUSTFLAGS", value);
        let out = f();
        drop(restore);
        out
    }

    #[test]
    fn verify_accepts_valid_schema_and_digests() {
        let manifest = b"manifest";
        let payload = b"payload";
        let sbom = b"sbom";
        let bytes = with_rustflags(String::new(), || {
            capture_bundle_repro_json(manifest, payload, sbom).expect("capture should work")
        });
        let expected = ReproVerifyInput {
            payload_sha256: sha256_hex(payload),
            manifest_sha256: sha256_hex(manifest),
            sbom_sha256: sha256_hex(sbom),
        };
        verify_repro_json(&bytes, &expected).expect("verify should pass");
    }

    #[test]
    fn verify_reject_repro_schema_invalid_extra_field() {
        let invalid = br#"{
            "schema_version":1,
            "source_date_epoch":0,
            "payload_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "manifest_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "sbom_sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            "rustc":"unknown",
            "target":"unknown",
            "rustflags":"",
            "unexpected":"field"
        }"#;
        let expected = ReproVerifyInput {
            payload_sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_string(),
            manifest_sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .to_string(),
            sbom_sha256: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                .to_string(),
        };
        let err = verify_repro_json(invalid, &expected).expect_err("schema must reject extra keys");
        assert!(matches!(err, ReproError::SchemaInvalid(_)));
    }

    #[test]
    fn verify_rejects_digest_mismatch() {
        let manifest = b"manifest";
        let payload = b"payload";
        let sbom = b"sbom";
        let bytes = with_rustflags(String::new(), || {
            capture_bundle_repro_json(manifest, payload, sbom).expect("capture should work")
        });
        let expected = ReproVerifyInput {
            payload_sha256: sha256_hex(b"other"),
            manifest_sha256: sha256_hex(manifest),
            sbom_sha256: sha256_hex(sbom),
        };
        let err =
            verify_repro_json(&bytes, &expected).expect_err("payload digest mismatch expected");
        assert!(matches!(err, ReproError::DigestMismatch { field: "payload_sha256" }));
    }

    #[test]
    fn capture_rejects_oversized_rustflags() {
        let err = with_rustflags("x".repeat(MAX_RUSTFLAGS_LEN + 1), || {
            capture_bundle_repro_json(b"manifest", b"payload", b"sbom").expect_err("size cap")
        });
        assert!(matches!(err, ReproError::InputTooLarge { field: "rustflags", .. }));
    }
}
