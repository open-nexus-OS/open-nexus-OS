//! CONTEXT: Deterministic CycloneDX SBOM generator for `.nxb` bundles
//! OWNERS: @runtime @security
//! STATUS: Functional
//! API_STABILITY: Unstable (internal tooling contract)
//! TEST_COVERAGE: 4 unit tests
//!   - determinism_same_input_same_output
//!   - reject_secret_like_content
//!   - reject_too_many_components
//!   - emits_cyclonedx_bom_ref_keys
//!
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md
#![forbid(unsafe_code)]

use nexus_evidence::{scan_for_secrets_with, test_support, EvidenceError, ScanAllowlist};
use serde::Serialize;
use sha1::{Digest, Sha1};
use thiserror::Error;

const SBOM_TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_BUNDLE_NAME_LEN: usize = 128;
const MAX_BUNDLE_VERSION_LEN: usize = 64;
const MAX_COMPONENTS: usize = 512;
const MAX_COMPONENT_FIELD_LEN: usize = 256;
const MAX_SBOM_JSON_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SbomComponentInput {
    pub name: String,
    pub version: String,
    pub purl: Option<String>,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleSbomInput {
    pub bundle_name: String,
    pub bundle_version: String,
    pub publisher_hex: String,
    pub payload_sha256: String,
    pub payload_size: u64,
    pub manifest_sha256: String,
    pub source_date_epoch: u64,
    pub components: Vec<SbomComponentInput>,
}

#[derive(Debug, Error)]
pub enum SbomError {
    #[error("invalid publisher hex: expected 16 bytes")]
    InvalidPublisherHex,
    #[error("invalid SHA-256 hex for `{field}`")]
    InvalidSha256Hex { field: &'static str },
    #[error("invalid SOURCE_DATE_EPOCH: {0}")]
    InvalidSourceDateEpoch(String),
    #[error("failed to format timestamp: {0}")]
    TimestampFormat(String),
    #[error("SBOM secret scan failed: pattern={pattern} artifact={artifact} line={line}")]
    SecretLeak { pattern: &'static str, artifact: &'static str, line: usize },
    #[error("SBOM secret scan failed: {0}")]
    SecretScanner(String),
    #[error("failed to serialize SBOM JSON: {0}")]
    Serialize(String),
    #[error("input too large: {field} (max={max}, actual={actual})")]
    InputTooLarge { field: &'static str, max: usize, actual: usize },
}

pub fn source_date_epoch_from_env() -> Result<u64, SbomError> {
    match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(raw) => raw.parse::<u64>().map_err(|_| SbomError::InvalidSourceDateEpoch(raw)),
        Err(std::env::VarError::NotPresent) => Ok(0),
        Err(err) => Err(SbomError::InvalidSourceDateEpoch(err.to_string())),
    }
}

pub fn generate_bundle_sbom_json(input: &BundleSbomInput) -> Result<Vec<u8>, SbomError> {
    validate_input(input)?;
    let timestamp = format_timestamp(input.source_date_epoch)?;
    let serial_number = uuid_v5_like(input);
    let mut components = input.components.clone();
    components.sort_by(|a, b| (&a.name, &a.version, &a.purl).cmp(&(&b.name, &b.version, &b.purl)));

    let root = CycloneDx {
        bom_format: "CycloneDX",
        spec_version: "1.5",
        serial_number: format!("urn:uuid:{serial_number}"),
        version: 1,
        metadata: Metadata {
            timestamp,
            tools: vec![Tool {
                vendor: "open-nexus-os".to_string(),
                name: "sbom".to_string(),
                version: SBOM_TOOL_VERSION.to_string(),
            }],
            component: BundleComponent {
                kind: "application",
                bom_ref: format!("bundle:{}@{}", input.bundle_name, input.bundle_version),
                name: input.bundle_name.clone(),
                version: input.bundle_version.clone(),
                publisher: input.publisher_hex.clone(),
                hashes: vec![HashEntry { alg: "SHA-256", content: input.payload_sha256.clone() }],
            },
        },
        components: components
            .into_iter()
            .map(|component| Component {
                kind: "library",
                bom_ref: format!("component:{}@{}", component.name, component.version),
                name: component.name,
                version: component.version,
                purl: component.purl,
                hashes: component
                    .sha256
                    .into_iter()
                    .map(|sha| HashEntry { alg: "SHA-256", content: sha })
                    .collect(),
            })
            .collect(),
        properties: vec![
            Property { name: "nexus.publisher.id".to_string(), value: input.publisher_hex.clone() },
            Property {
                name: "nexus.payload.sha256".to_string(),
                value: input.payload_sha256.clone(),
            },
            Property {
                name: "nexus.payload.bytes".to_string(),
                value: input.payload_size.to_string(),
            },
            Property {
                name: "nexus.manifest.sha256".to_string(),
                value: input.manifest_sha256.clone(),
            },
        ],
    };

    let bytes = serde_json::to_vec(&root).map_err(|err| SbomError::Serialize(err.to_string()))?;
    if bytes.len() > MAX_SBOM_JSON_BYTES {
        return Err(SbomError::InputTooLarge {
            field: "sbom_json",
            max: MAX_SBOM_JSON_BYTES,
            actual: bytes.len(),
        });
    }
    let allowlist = build_allowlist(input)?;
    reject_if_secret_leak(&bytes, &allowlist)?;
    Ok(bytes)
}

fn validate_input(input: &BundleSbomInput) -> Result<(), SbomError> {
    if input.bundle_name.len() > MAX_BUNDLE_NAME_LEN {
        return Err(SbomError::InputTooLarge {
            field: "bundle_name",
            max: MAX_BUNDLE_NAME_LEN,
            actual: input.bundle_name.len(),
        });
    }
    if input.bundle_version.len() > MAX_BUNDLE_VERSION_LEN {
        return Err(SbomError::InputTooLarge {
            field: "bundle_version",
            max: MAX_BUNDLE_VERSION_LEN,
            actual: input.bundle_version.len(),
        });
    }
    if input.components.len() > MAX_COMPONENTS {
        return Err(SbomError::InputTooLarge {
            field: "components",
            max: MAX_COMPONENTS,
            actual: input.components.len(),
        });
    }
    if hex::decode(&input.publisher_hex).map(|bytes| bytes.len() != 16).unwrap_or(true) {
        return Err(SbomError::InvalidPublisherHex);
    }
    validate_sha256(&input.payload_sha256, "payload_sha256")?;
    validate_sha256(&input.manifest_sha256, "manifest_sha256")?;
    for component in &input.components {
        if component.name.len() > MAX_COMPONENT_FIELD_LEN {
            return Err(SbomError::InputTooLarge {
                field: "component.name",
                max: MAX_COMPONENT_FIELD_LEN,
                actual: component.name.len(),
            });
        }
        if component.version.len() > MAX_COMPONENT_FIELD_LEN {
            return Err(SbomError::InputTooLarge {
                field: "component.version",
                max: MAX_COMPONENT_FIELD_LEN,
                actual: component.version.len(),
            });
        }
        if let Some(purl) = component.purl.as_deref() {
            if purl.len() > MAX_COMPONENT_FIELD_LEN {
                return Err(SbomError::InputTooLarge {
                    field: "component.purl",
                    max: MAX_COMPONENT_FIELD_LEN,
                    actual: purl.len(),
                });
            }
        }
        if let Some(sha) = component.sha256.as_deref() {
            validate_sha256(sha, "component.sha256")?;
        }
    }
    Ok(())
}

fn validate_sha256(value: &str, field: &'static str) -> Result<(), SbomError> {
    if value.len() != 64 {
        return Err(SbomError::InvalidSha256Hex { field });
    }
    let decoded = hex::decode(value).map_err(|_| SbomError::InvalidSha256Hex { field })?;
    if decoded.len() != 32 {
        return Err(SbomError::InvalidSha256Hex { field });
    }
    Ok(())
}

fn format_timestamp(source_date_epoch: u64) -> Result<String, SbomError> {
    let secs_per_day = 86_400u64;
    let days = source_date_epoch / secs_per_day;
    let sec_of_day = source_date_epoch % secs_per_day;
    let days_i64 = i64::try_from(days)
        .map_err(|_| SbomError::TimestampFormat("epoch too large".to_string()))?;
    let (year, month, day) = civil_from_days(days_i64)?;
    let hour = sec_of_day / 3_600;
    let minute = (sec_of_day % 3_600) / 60;
    let second = sec_of_day % 60;
    Ok(format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"))
}

fn civil_from_days(days_since_unix_epoch: i64) -> Result<(i32, u32, u32), SbomError> {
    // Howard Hinnant's civil-from-days algorithm.
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };

    let year_i32 =
        i32::try_from(year).map_err(|_| SbomError::TimestampFormat("year overflow".to_string()))?;
    let month_u32 = u32::try_from(month)
        .map_err(|_| SbomError::TimestampFormat("month overflow".to_string()))?;
    let day_u32 =
        u32::try_from(day).map_err(|_| SbomError::TimestampFormat("day overflow".to_string()))?;
    Ok((year_i32, month_u32, day_u32))
}

fn uuid_v5_like(input: &BundleSbomInput) -> String {
    let mut hasher = Sha1::new();
    hasher.update(b"open-nexus-os:sbom:v1:");
    hasher.update(input.bundle_name.as_bytes());
    hasher.update(b":");
    hasher.update(input.bundle_version.as_bytes());
    hasher.update(b":");
    hasher.update(input.payload_sha256.as_bytes());
    let digest = hasher.finalize();

    let mut uuid = [0u8; 16];
    uuid.copy_from_slice(&digest[..16]);
    uuid[6] = (uuid[6] & 0x0f) | 0x50;
    uuid[8] = (uuid[8] & 0x3f) | 0x80;

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        uuid[0],
        uuid[1],
        uuid[2],
        uuid[3],
        uuid[4],
        uuid[5],
        uuid[6],
        uuid[7],
        uuid[8],
        uuid[9],
        uuid[10],
        uuid[11],
        uuid[12],
        uuid[13],
        uuid[14],
        uuid[15]
    )
}

fn build_allowlist(input: &BundleSbomInput) -> Result<ScanAllowlist, SbomError> {
    let mut fragments = vec![
        input.publisher_hex.clone(),
        input.payload_sha256.clone(),
        input.manifest_sha256.clone(),
    ];
    for component in &input.components {
        if let Some(sha) = component.sha256.as_ref() {
            fragments.push(sha.clone());
        }
    }
    let escaped = fragments
        .into_iter()
        .map(|fragment| fragment.replace('\\', "\\\\").replace('"', "\\\""))
        .collect::<Vec<_>>()
        .join("\",\"");
    let toml = format!("[allowlist]\nsubstrings=[\"{escaped}\"]\n");
    ScanAllowlist::from_toml(&toml).map_err(|err| SbomError::SecretScanner(err.to_string()))
}

fn reject_if_secret_leak(bytes: &[u8], allowlist: &ScanAllowlist) -> Result<(), SbomError> {
    let mut bundle = test_support::empty_bundle();
    bundle.uart.bytes = bytes.to_vec();
    scan_for_secrets_with(&bundle, allowlist).map_err(|err| match err {
        EvidenceError::SecretLeak { artifact, line, pattern } => {
            SbomError::SecretLeak { artifact, line, pattern }
        }
        other => SbomError::SecretScanner(other.to_string()),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CycloneDx {
    #[serde(rename = "bomFormat")]
    bom_format: &'static str,
    spec_version: &'static str,
    serial_number: String,
    version: u8,
    metadata: Metadata,
    components: Vec<Component>,
    properties: Vec<Property>,
}

#[derive(Debug, Serialize)]
struct Metadata {
    timestamp: String,
    tools: Vec<Tool>,
    component: BundleComponent,
}

#[derive(Debug, Serialize)]
struct Tool {
    vendor: String,
    name: String,
    version: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BundleComponent {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(rename = "bom-ref")]
    bom_ref: String,
    name: String,
    version: String,
    publisher: String,
    hashes: Vec<HashEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Component {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(rename = "bom-ref")]
    bom_ref: String,
    name: String,
    version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    purl: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    hashes: Vec<HashEntry>,
}

#[derive(Debug, Serialize)]
struct HashEntry {
    #[serde(rename = "alg")]
    alg: &'static str,
    #[serde(rename = "content")]
    content: String,
}

#[derive(Debug, Serialize)]
struct Property {
    name: String,
    value: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> BundleSbomInput {
        BundleSbomInput {
            bundle_name: "demo.app".to_string(),
            bundle_version: "1.0.0".to_string(),
            publisher_hex: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            payload_sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .to_string(),
            payload_size: 4096,
            manifest_sha256: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                .to_string(),
            source_date_epoch: 1_700_000_000,
            components: vec![SbomComponentInput {
                name: "serde".to_string(),
                version: "1.0.0".to_string(),
                purl: Some("pkg:cargo/serde@1.0.0".to_string()),
                sha256: None,
            }],
        }
    }

    #[test]
    fn determinism_same_input_same_output() {
        let first = generate_bundle_sbom_json(&input()).expect("first generation should succeed");
        let second = generate_bundle_sbom_json(&input()).expect("second generation should succeed");
        assert_eq!(first, second);
    }

    #[test]
    fn reject_secret_like_content() {
        let mut bad = input();
        bad.components = vec![SbomComponentInput {
            name: "-----BEGIN PRIVATE KEY-----".to_string(),
            version: "1.0.0".to_string(),
            purl: None,
            sha256: None,
        }];
        let err = generate_bundle_sbom_json(&bad).expect_err("secret scanner should reject");
        assert!(matches!(err, SbomError::SecretLeak { .. }));
    }

    #[test]
    fn reject_too_many_components() {
        let mut bad = input();
        bad.components = (0..(MAX_COMPONENTS + 1))
            .map(|idx| SbomComponentInput {
                name: format!("crate-{idx}"),
                version: "1.0.0".to_string(),
                purl: None,
                sha256: None,
            })
            .collect();
        let err = generate_bundle_sbom_json(&bad).expect_err("component cap should reject");
        assert!(matches!(err, SbomError::InputTooLarge { field: "components", .. }));
    }

    #[test]
    fn emits_cyclonedx_bom_ref_keys() {
        let json = generate_bundle_sbom_json(&input()).expect("sbom generation");
        let value: serde_json::Value = serde_json::from_slice(&json).expect("parse sbom json");
        let component = value
            .get("metadata")
            .and_then(|meta| meta.get("component"))
            .and_then(|component| component.as_object())
            .expect("metadata.component object");
        assert!(component.contains_key("bom-ref"));
        assert!(!component.contains_key("bomRef"));

        let lib = value
            .get("components")
            .and_then(|components| components.as_array())
            .and_then(|components| components.first())
            .and_then(|component| component.as_object())
            .expect("first components entry");
        assert!(lib.contains_key("bom-ref"));
        assert!(!lib.contains_key("bomRef"));
    }
}
