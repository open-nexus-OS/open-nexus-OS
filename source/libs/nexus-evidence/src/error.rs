// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Stable error categories for the evidence-bundle pipeline.
//! Variants are append-only across cuts (Phase-4 invariant carried
//! forward from `nexus-proof-manifest`); never rename, never remove.
//!
//! P5-01 surface: assembly + canonical-hash facing variants only.
//! Sign/verify variants land in P5-03; secret-scanner variants in
//! P5-04. Operational gate variants (P5-05) reuse the existing
//! categories rather than adding new ones.
//!
//! OWNERS: @runtime
//! STATUS: Functional (P5-01 surface)
//! API_STABILITY: Unstable (Phase 5 evolves variants between cuts)
//! TEST_COVERAGE: see `tests/canonical_hash.rs` (P5-01)

use core::fmt;

/// Stable error categories for the evidence-bundle pipeline.
///
/// Each variant maps to a documented exit code in
/// `docs/testing/evidence-bundle.md` §"Error class table". Future cuts
/// (P5-03 sign/verify, P5-04 secret scanner) extend this enum; existing
/// variants never change name or semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceError {
    /// A required artifact field on [`crate::Bundle`] is empty when
    /// downstream demand requires it. P5-01 reserves this variant; the
    /// canonical-hash path tolerates empty artifacts (an empty bundle
    /// hashes deterministically). Bundle-assembly (P5-02) wires it.
    MissingArtifact {
        /// Name of the artifact (`manifest`, `uart`, `trace`, `config`).
        artifact: &'static str,
    },
    /// A `TraceEntry` violates schema invariants. P5-02 will refine the
    /// payload to include line number + cause; reserved here so that
    /// the variant set is complete from the start.
    MalformedTrace {
        /// Brief, stable diagnostic (e.g. `"unknown_marker"`).
        detail: String,
    },
    /// A `ConfigArtifact` field violates schema invariants. P5-02 will
    /// refine the payload with the offending field name.
    MalformedConfig {
        /// Brief, stable diagnostic (e.g. `"empty_profile"`).
        detail: String,
    },
    /// Internal serialization failure during canonicalization. Should
    /// never fire on well-formed inputs; reserved for defensive paths.
    CanonicalizationFailed {
        /// Brief, stable diagnostic.
        detail: String,
    },
    /// `BundleMeta::schema_version` is not in the parser's accepted
    /// set. P5-01 accepts only `1`. Future cuts may bump to `2`.
    SchemaVersionUnsupported {
        /// The unsupported version observed.
        version: u8,
    },
    /// `verify` was called on a bundle that has no `signature.bin`.
    /// Distinct from [`Self::SignatureMismatch`] so that callers can
    /// distinguish "unsigned bundle" from "tamper" in their exit
    /// codes / diagnostics. (P5-03)
    SignatureMissing,
    /// `signature.bin` exists but does not parse: bad magic, bad
    /// length, unknown label byte, or any structural failure short
    /// of the version byte. (P5-03)
    SignatureMalformed {
        /// Brief, stable diagnostic (e.g. `"bad_magic"`, `"bad_length"`).
        detail: String,
    },
    /// `signature.bin` parsed but either the embedded canonical
    /// hash does not match the recomputed hash (= bundle was
    /// tampered after signing), or the Ed25519 verify call failed
    /// (= bundle was signed by a different key than the verifier
    /// holds). (P5-03)
    SignatureMismatch {
        /// Brief, stable diagnostic (e.g. `"canonical_hash_changed"`).
        detail: String,
    },
    /// Verifier policy demands a specific [`crate::key::KeyLabel`]
    /// (e.g. `--policy=ci`) but the bundle was sealed under a
    /// different one. The signature itself is valid; the bundle is
    /// just not acceptable under the requested policy. (P5-03)
    KeyLabelMismatch {
        /// Label the verifier policy demands.
        expected: &'static str,
        /// Label that actually signed the bundle.
        got: &'static str,
    },
    /// `signature.bin` carries a wire-format version this parser
    /// does not understand. Bumping the version byte is a hard
    /// break: old verifiers MUST refuse new bundles rather than
    /// silently accept a possibly-incompatible payload. (P5-03)
    UnsupportedSignatureVersion {
        /// Version byte observed in the on-disk signature.
        got: u8,
        /// Version byte this build understands.
        supported: u8,
    },
    /// Secret-scan rejected an artifact before sealing. The signed
    /// bundle would have committed key/credential material to the
    /// evidence stream; sealing was refused. (P5-04)
    SecretLeak {
        /// Bundle artifact that contained the leak (`uart.log`,
        /// `trace.jsonl`, `config.json`).
        artifact: &'static str,
        /// 1-indexed line number within the artifact's textual
        /// projection (UART line number, trace entry index,
        /// config field synthetic line).
        line: usize,
        /// Stable [`crate::scan::LeakKind::as_str`] label.
        pattern: &'static str,
    },
    /// `KeyLabel::from_env_or_dir` could not locate a usable
    /// private key in either the CI env source or the bringup file
    /// source. (P5-04)
    KeyMaterialMissing,
    /// Bringup `private.ed25519` exists but its on-disk
    /// permissions are not `0600`. Refusing the key prevents an
    /// unintentional leak via a world-readable file. (P5-04)
    KeyMaterialPermissions {
        /// Path of the offending key file.
        path: String,
        /// Octal mode actually observed on disk.
        mode: u32,
    },
}

impl fmt::Display for EvidenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvidenceError::MissingArtifact { artifact } => {
                write!(f, "evidence: missing artifact `{}`", artifact)
            }
            EvidenceError::MalformedTrace { detail } => {
                write!(f, "evidence: malformed trace: {}", detail)
            }
            EvidenceError::MalformedConfig { detail } => {
                write!(f, "evidence: malformed config: {}", detail)
            }
            EvidenceError::CanonicalizationFailed { detail } => {
                write!(f, "evidence: canonicalization failed: {}", detail)
            }
            EvidenceError::SchemaVersionUnsupported { version } => {
                write!(
                    f,
                    "evidence: bundle schema_version={} not supported (P5-01 accepts only 1)",
                    version
                )
            }
            EvidenceError::SignatureMissing => {
                write!(f, "evidence: signature missing (bundle is unsigned)")
            }
            EvidenceError::SignatureMalformed { detail } => {
                write!(f, "evidence: signature malformed: {}", detail)
            }
            EvidenceError::SignatureMismatch { detail } => {
                write!(f, "evidence: signature mismatch: {}", detail)
            }
            EvidenceError::KeyLabelMismatch { expected, got } => {
                write!(
                    f,
                    "evidence: key label mismatch: policy expects `{}`, bundle was sealed with `{}`",
                    expected, got
                )
            }
            EvidenceError::UnsupportedSignatureVersion { got, supported } => {
                write!(
                    f,
                    "evidence: signature version 0x{:02x} not supported (this build understands 0x{:02x})",
                    got, supported
                )
            }
            EvidenceError::SecretLeak {
                artifact,
                line,
                pattern,
            } => {
                write!(
                    f,
                    "evidence: secret leak in `{}` line {}: pattern={}",
                    artifact, line, pattern
                )
            }
            EvidenceError::KeyMaterialMissing => {
                write!(
                    f,
                    "evidence: key material missing (set NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64 or create ~/.config/nexus/bringup-key/private.ed25519)"
                )
            }
            EvidenceError::KeyMaterialPermissions { path, mode } => {
                write!(
                    f,
                    "evidence: key material `{}` has perms 0{:o} (want 0600)",
                    path, mode
                )
            }
        }
    }
}

impl std::error::Error for EvidenceError {}
