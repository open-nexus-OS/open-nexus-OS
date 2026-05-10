// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Host-only crate that owns the evidence-bundle schema and
//! the canonical hash for the `selftest-client` Phase-5 deliverable.
//! A bundle binds five artifacts (manifest + uart + trace + config +
//! signature) into a portable, attestable record of one QEMU run.
//!
//! Cut layout (incremental; this cut = **P5-01**):
//!   - **P5-01** (this cut): schema types ([`Bundle`], [`BundleMeta`],
//!     [`ManifestArtifact`], [`UartArtifact`], [`TraceArtifact`],
//!     [`TraceEntry`], [`ConfigArtifact`], [`Signature`]) +
//!     [`canonical_hash`] over the 4 hashed artifacts. No bundle
//!     assembly, no I/O, no signing.
//!   - P5-02: `Bundle::assemble` + trace extractor + config builder +
//!     `nexus-evidence` CLI (`assemble`/`inspect`/`canonical-hash`).
//!   - P5-03: Ed25519 sign/verify + 5 tamper classes.
//!   - P5-04: CI/bringup key separation + secret scanner.
//!   - P5-05: harness post-pass seal integration + CI gates.
//!
//! Non-goal: this crate MUST NOT enter the
//! `selftest-client --features os-lite` graph. Forbidden runtime deps
//! (`getrandom`, `parking_lot`, `parking_lot_core`) stay forbidden in
//! the OS slice; this crate is host-only.
//!
//! Spec: [`docs/testing/evidence-bundle.md`](../../docs/testing/evidence-bundle.md)
//! is the normative source; the Rust types here mirror it.
//!
//! OWNERS: @runtime
//! STATUS: Functional (P5-01 surface)
//! API_STABILITY: Unstable (Phase 5 evolves shape between cuts)
//! TEST_COVERAGE: see `tests/canonical_hash.rs` (6 tests)

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod bundle_io;
mod canonical;
mod config;
mod error;
pub mod key;
pub mod scan;
mod trace;

pub use bundle_io::{pack_manifest_tree, read_unsigned, write_unsigned};
pub use config::{gather_config, GatherOpts};
pub use error::EvidenceError;
pub use key::{KeyLabel, Signature, SigningKey, VerifyingKey};
pub use scan::{scan_for_secrets, scan_for_secrets_with, LeakKind, ScanAllowlist};
pub use trace::extract_trace;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use glob::glob;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use toml::Value as TomlValue;

/// A complete evidence bundle for a single QEMU run.
///
/// The five fields mirror the on-tar layout described in
/// `docs/testing/evidence-bundle.md` §1. The bundle is never
/// serialized as a single blob — each artifact is written to its
/// own tarball entry by [`bundle_io`]. The artifact subtypes derive
/// `serde` for that path; `Bundle` itself does not.
#[derive(Debug, Clone)]
pub struct Bundle {
    /// Bundle-level metadata (schema version + profile name).
    pub meta: BundleMeta,
    /// Verbatim copy of the manifest bytes the run sealed against.
    pub manifest: ManifestArtifact,
    /// Raw serial output (`uart.log`).
    pub uart: UartArtifact,
    /// Extracted marker ladder; one entry per emitted marker.
    pub trace: TraceArtifact,
    /// Run configuration (profile, env, kernel cmdline, host info...).
    pub config: ConfigArtifact,
    /// Optional signature over [`canonical_hash`]. P5-01 always sets
    /// this to `None`; P5-03 introduces the seal/verify path.
    pub signature: Option<Signature>,
}

/// Bundle-level metadata. Hashed first in [`canonical_hash`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleMeta {
    /// Bundle schema version. **Phase-5 ships `1`.** Append-only enum.
    pub schema_version: u8,
    /// Profile name as accepted by `nexus-proof-manifest` (e.g. `full`).
    pub profile: String,
}

/// Verbatim manifest bytes packed into the bundle.
///
/// For a v1 manifest this is the single TOML file's bytes. For a v2
/// manifest (P5-00 onward) this is a deterministic tar of the split
/// tree (file order = lexicographic; `mtime=0`); the inner-tar bytes
/// are what gets hashed. P5-02 finalizes the inner-tar layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestArtifact {
    /// Raw bytes hashed by [`canonical_hash`] without further
    /// transformation. The manifest is the source of truth for itself.
    pub bytes: Vec<u8>,
}

/// Raw serial output (`uart.log`).
///
/// Stored as raw bytes; line-ending normalization (`\r\n` → `\n`)
/// happens at hash-time only — see
/// `docs/testing/evidence-bundle.md` §3.2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UartArtifact {
    /// Raw bytes as captured from the QEMU serial stream.
    pub bytes: Vec<u8>,
}

/// One extracted marker line from `trace.jsonl`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceEntry {
    /// The exact UART marker literal (e.g. `"SELFTEST: vfs ok"`).
    pub marker: String,
    /// The manifest phase the marker belongs to. Looked up via the
    /// manifest at extraction time, NOT inferred from UART position.
    pub phase: String,
    /// `[ts=…ms]` prefix value when present in the UART line.
    /// `None` is allowed: not every emit site carries a timestamp yet.
    pub ts_ms_from_boot: Option<u64>,
    /// Profile this entry was recorded under. Constant per bundle.
    pub profile: String,
}

/// Sequence of [`TraceEntry`] values, one per emitted marker line.
///
/// Order reflects extraction order at assembly time. The canonical
/// hash sorts internally; in-memory reordering does NOT change the
/// hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TraceArtifact {
    /// Trace entries in extraction order.
    pub entries: Vec<TraceEntry>,
}

/// Run configuration captured at seal time.
///
/// `wall_clock_utc` is the only field excluded from
/// [`canonical_hash`] — see `docs/testing/evidence-bundle.md` §3.2.
/// Re-sealing the same run produces a bundle with a different
/// `wall_clock_utc` but the same root hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigArtifact {
    /// Profile name (echoes [`BundleMeta::profile`]).
    pub profile: String,
    /// Resolved profile env (output of `nexus-proof-manifest list-env`).
    /// `BTreeMap` enforces lexicographic key order, which is what the
    /// canonical hash depends on for env-key-order-invariance.
    pub env: BTreeMap<String, String>,
    /// Kernel command line passed via `-append`.
    pub kernel_cmdline: String,
    /// QEMU argv (order is significant; preserved as captured).
    pub qemu_args: Vec<String>,
    /// `uname -a` (single line) of the host that ran QEMU.
    pub host_info: String,
    /// `git rev-parse HEAD` at run time.
    pub build_sha: String,
    /// `rustc --version` of the toolchain that built the OS image.
    pub rustc_version: String,
    /// First line of `qemu-system-riscv64 --version`.
    pub qemu_version: String,
    /// RFC-3339 UTC timestamp of seal time. **Excluded from the hash**;
    /// reproducibility carve-out so two reseals yield the same root.
    pub wall_clock_utc: String,
}

/// Compute the canonical 32-byte SHA-256 root hash of a bundle.
///
/// Formula (frozen at P5-01; see `docs/testing/evidence-bundle.md` §3.1):
///
/// ```text
/// H_root = SHA256(
///   H(meta_canonical) ||
///   H(manifest.bytes) ||
///   H(uart_normalized) ||
///   H(trace_canonical) ||
///   H(config_canonical))
/// ```
///
/// The five intermediate hashes are concatenated in fixed order
/// (meta, manifest, uart, trace, config) and hashed once more.
///
/// This function is pure: it takes only the [`Bundle`] and returns a
/// 32-byte digest. It does NOT depend on `bundle.signature` (the
/// signature signs *this* hash, so it can't include itself).
pub fn canonical_hash(bundle: &Bundle) -> [u8; 32] {
    let h_meta = sha256(&canonical::serialize_meta_canonical(&bundle.meta));
    let h_manifest = sha256(&bundle.manifest.bytes);
    let h_uart = sha256(&canonical::normalize_line_endings(&bundle.uart.bytes));
    let h_trace = sha256(&canonical::serialize_trace_canonical(&bundle.trace.entries));
    let h_config = sha256(&canonical::serialize_config_canonical(&bundle.config));

    let mut root = Sha256::new();
    root.update(h_meta);
    root.update(h_manifest);
    root.update(h_uart);
    root.update(h_trace);
    root.update(h_config);
    let out = root.finalize();
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&out);
    bytes
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    let out = h.finalize();
    let mut buf = [0u8; 32];
    buf.copy_from_slice(&out);
    buf
}

// ---------------------------------------------------------------------------
// Bundle assembly (P5-02)
// ---------------------------------------------------------------------------

/// Inputs to [`Bundle::assemble`].
///
/// Bundle assembly reads three on-disk inputs (UART transcript,
/// manifest tree, gather options) and produces a fully-populated
/// [`Bundle`] in memory. Writing it to disk is a separate step
/// ([`write_unsigned`]); sealing is yet another (P5-03).
///
/// Field-by-field rationale:
///   - `uart_path`: read raw, line-ending-normalized at hash-time
///     only.
///   - `manifest_path`: parsed via [`nexus_proof_manifest::parse`] after
///     normalizing v2 split-layout manifests into a deterministic v1-style
///     source for compatibility. The crate then packs every resolved
///     manifest source file into the inner `manifest.tar` (deterministic
///     order, mtime=0).
///   - `gather_opts`: caller-supplied host-introspection results
///     (see [`GatherOpts`]). The crate does NOT shell out itself.
#[derive(Debug, Clone)]
pub struct AssembleOpts {
    /// Path to the run's `uart.log`.
    pub uart_path: PathBuf,
    /// Path to the proof-manifest root (`*.toml` for v1; the v2
    /// `proof-manifest/manifest.toml` for v2).
    pub manifest_path: PathBuf,
    /// Already-collected host inputs and resolved env. The
    /// `profile` field MUST match `gather_opts.profile`.
    pub gather_opts: GatherOpts,
}

/// Parse the proof-manifest from disk via the host parser crate.
///
/// Keeping this in a dedicated helper makes the evidence assembly path's
/// parser dependency explicit and easier to smoke-test.
fn parse_manifest_from_path(path: &Path) -> Result<nexus_proof_manifest::Manifest, EvidenceError> {
    let source =
        std::fs::read_to_string(path).map_err(|e| EvidenceError::CanonicalizationFailed {
            detail: format!("read manifest {}: {e}", path.display()),
        })?;
    let normalized = normalize_manifest_source(path, &source)?;
    nexus_proof_manifest::parse(&normalized).map_err(|e| EvidenceError::CanonicalizationFailed {
        detail: format!("parse_manifest({}): {e}", path.display()),
    })
}

impl Bundle {
    /// Assemble a complete (unsigned) bundle from the inputs in
    /// `opts`. The returned bundle has `signature = None`; sealing
    /// (P5-03) is a separate step.
    ///
    /// # Errors
    ///
    /// - [`EvidenceError::MissingArtifact`] if the UART file or the
    ///   manifest path can't be read.
    /// - [`EvidenceError::MalformedTrace`] if the trace extractor
    ///   rejects a UART line (unknown marker, malformed `[ts=…ms]`).
    /// - [`EvidenceError::MalformedConfig`] if `opts.gather_opts`
    ///   carries an empty profile.
    /// - [`EvidenceError::CanonicalizationFailed`] for tar-packing
    ///   I/O failures.
    pub fn assemble(opts: AssembleOpts) -> Result<Bundle, EvidenceError> {
        let uart_bytes = std::fs::read(&opts.uart_path).map_err(|e| {
            EvidenceError::MissingArtifact { artifact: "uart" }.with_context(format!(
                "read {}: {}",
                opts.uart_path.display(),
                e
            ))
        })?;
        let uart_text = String::from_utf8_lossy(&uart_bytes).into_owned();

        let manifest = parse_manifest_from_path(&opts.manifest_path)?;

        let profile = opts.gather_opts.profile.clone();
        let trace_entries = extract_trace(&uart_text, &manifest, &profile)?;
        let config = gather_config(opts.gather_opts)?;

        let manifest_files = collect_manifest_files(&opts.manifest_path)?;
        let manifest_tar_bytes = pack_manifest_tree(&manifest_files)?;

        Ok(Bundle {
            meta: BundleMeta {
                schema_version: 1,
                profile,
            },
            manifest: ManifestArtifact {
                bytes: manifest_tar_bytes,
            },
            uart: UartArtifact { bytes: uart_bytes },
            trace: TraceArtifact {
                entries: trace_entries,
            },
            config,
            signature: None,
        })
    }

    /// Write `self` to `path` as a `tar.gz`. The bundle's
    /// [`Signature`] field — when populated — lands as
    /// `signature.bin` inside the archive. The function name is
    /// historical (P5-02 only handled the unsigned case); from
    /// P5-03 it is the unconditional write entry point.
    pub fn write_unsigned(&self, path: &Path) -> Result<(), EvidenceError> {
        write_unsigned(self, path)
    }

    /// Produce a sealed bundle by signing the canonical hash of
    /// `self` with `signing_key`.
    ///
    /// As of P5-04 this method **always** runs the secret scanner
    /// (deny-by-default) before signing — a bundle that would
    /// commit secret material to the evidence stream cannot be
    /// sealed. Callers that need an escape hatch (e.g. tests
    /// against a synthetic UART that intentionally embeds a
    /// fixture private-key block) use [`Bundle::seal_with`] with
    /// an allowlist.
    ///
    /// The returned bundle is a clone of `self` with
    /// `signature = Some(...)` populated; the canonical hash is
    /// unchanged (the signature does not feed back into the hash,
    /// so seal/verify is order-stable). The `label` is encoded
    /// into the signature byte stream so downstream verifiers can
    /// enforce key-class policy via [`Bundle::verify`].
    pub fn seal(&self, signing_key: &SigningKey, label: KeyLabel) -> Result<Bundle, EvidenceError> {
        self.seal_with(signing_key, label, &ScanAllowlist::empty())
    }

    /// Same as [`Bundle::seal`] but takes a scan allowlist.
    /// Tests use this to suppress the high-entropy heuristic on
    /// known-benign synthetic fixtures; production callers stick
    /// with [`Bundle::seal`].
    pub fn seal_with(
        &self,
        signing_key: &SigningKey,
        label: KeyLabel,
        allowlist: &ScanAllowlist,
    ) -> Result<Bundle, EvidenceError> {
        scan::scan_for_secrets_with(self, allowlist)?;
        let h = canonical_hash(self);
        let sig = signing_key.sign_hash(h, label);
        let mut out = self.clone();
        out.signature = Some(sig);
        Ok(out)
    }

    /// Verify `self` against `verifying_key`.
    ///
    /// `policy` (when `Some`) restricts the acceptable
    /// [`KeyLabel`]; e.g. a CI gate calls `verify(&pubkey,
    /// Some(KeyLabel::Ci))` to refuse bringup-signed bundles.
    /// `None` means "any label is fine".
    ///
    /// # Errors
    ///
    /// - [`EvidenceError::SignatureMissing`] if the bundle is
    ///   unsigned.
    /// - [`EvidenceError::SignatureMismatch`] if the embedded
    ///   hash does not match the recomputed canonical hash
    ///   (= bundle was tampered after signing) **or** if the
    ///   Ed25519 verify call fails (= signed by a different key).
    /// - [`EvidenceError::KeyLabelMismatch`] if `policy` is set
    ///   and disagrees with the bundle's signature label.
    pub fn verify(
        &self,
        verifying_key: &VerifyingKey,
        policy: Option<KeyLabel>,
    ) -> Result<(), EvidenceError> {
        let sig = self
            .signature
            .as_ref()
            .ok_or(EvidenceError::SignatureMissing)?;
        if let Some(want) = policy {
            if sig.label != want {
                return Err(EvidenceError::KeyLabelMismatch {
                    expected: want.as_str(),
                    got: sig.label.as_str(),
                });
            }
        }
        let h = canonical_hash(self);
        verifying_key.verify(sig, &h)
    }

    /// Compact human-readable summary used by the `inspect` CLI
    /// subcommand. Format is **not** part of the canonical hash and
    /// may change between cuts.
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "bundle_schema_version: {}\n",
            self.meta.schema_version
        ));
        s.push_str(&format!("profile: {}\n", self.meta.profile));
        s.push_str(&format!("manifest_bytes: {}\n", self.manifest.bytes.len()));
        s.push_str(&format!("uart_bytes: {}\n", self.uart.bytes.len()));
        s.push_str(&format!("trace_entries: {}\n", self.trace.entries.len()));
        s.push_str(&format!(
            "config: profile={} env_keys={} qemu_args={} build_sha={}\n",
            self.config.profile,
            self.config.env.len(),
            self.config.qemu_args.len(),
            if self.config.build_sha.is_empty() {
                "<unset>"
            } else {
                &self.config.build_sha
            }
        ));
        s.push_str(&format!(
            "signature: {}\n",
            match &self.signature {
                Some(sig) => format!("present (label={})", sig.label.as_str()),
                None => "absent".to_string(),
            }
        ));
        s.push_str(&format!(
            "canonical_hash: {}\n",
            hex::encode(canonical_hash(self))
        ));
        s
    }
}

impl EvidenceError {
    /// Internal helper: replace a `MissingArtifact { artifact }` with
    /// a more specific `CanonicalizationFailed` carrying the I/O
    /// diagnostic. Avoids losing the cause when `read` fails for a
    /// reason other than "file truly absent".
    fn with_context(self, ctx: String) -> Self {
        match self {
            EvidenceError::MissingArtifact { artifact } => EvidenceError::CanonicalizationFailed {
                detail: format!("read {}: {}", artifact, ctx),
            },
            other => other,
        }
    }
}

/// Collect (relative-path, bytes) pairs for every manifest source file.
/// Paths are relativised
/// against the manifest root's parent directory so that v2 manifests
/// pack under their natural layout (`manifest.toml`, `phases.toml`,
/// `markers/<phase>.toml`, ...).
fn collect_manifest_files(manifest_path: &Path) -> Result<Vec<(String, Vec<u8>)>, EvidenceError> {
    let root_dir = manifest_path.parent().unwrap_or_else(|| Path::new(""));
    let sources = resolve_manifest_source_files(manifest_path)?;
    let mut out = Vec::with_capacity(sources.len());
    for src in &sources {
        let bytes = std::fs::read(src).map_err(|e| EvidenceError::CanonicalizationFailed {
            detail: format!("read manifest source {}: {}", src.display(), e),
        })?;
        let rel = src.strip_prefix(root_dir).unwrap_or(src);
        out.push((rel.to_string_lossy().into_owned(), bytes));
    }
    Ok(out)
}

fn normalize_manifest_source(manifest_path: &Path, source: &str) -> Result<String, EvidenceError> {
    let root = parse_toml_root(manifest_path, source)?;
    if !is_schema_v2(&root) {
        return Ok(source.to_string());
    }
    if root.get("phase").is_some() || root.get("profile").is_some() || root.get("marker").is_some()
    {
        return Err(EvidenceError::CanonicalizationFailed {
            detail: format!(
                "manifest {} mixes schema-v2 [include] with inline phase/profile/marker sections",
                manifest_path.display()
            ),
        });
    }

    let default_profile = root
        .get("meta")
        .and_then(TomlValue::as_table)
        .and_then(|m| m.get("default_profile"))
        .and_then(TomlValue::as_str)
        .ok_or_else(|| EvidenceError::CanonicalizationFailed {
            detail: format!(
                "manifest {} missing [meta].default_profile",
                manifest_path.display()
            ),
        })?;

    let include = root
        .get("include")
        .and_then(TomlValue::as_table)
        .ok_or_else(|| EvidenceError::CanonicalizationFailed {
            detail: format!(
                "manifest {} missing [include] table for schema v2",
                manifest_path.display()
            ),
        })?;

    let root_dir = manifest_path.parent().unwrap_or_else(|| Path::new(""));
    let mut merged = String::new();
    merged.push_str("[meta]\n");
    merged.push_str("schema_version = \"1\"\n");
    merged.push_str(&format!("default_profile = {:?}\n\n", default_profile));

    for category in ["phases", "markers", "profiles"] {
        let pattern = include_pattern(include, category, manifest_path)?;
        let files = expand_include_glob(root_dir, category, pattern, manifest_path)?;
        for file in files {
            let file_source = std::fs::read_to_string(&file).map_err(|e| {
                EvidenceError::CanonicalizationFailed {
                    detail: format!("read include {}: {e}", file.display()),
                }
            })?;
            merged.push_str(&file_source);
            if !file_source.ends_with('\n') {
                merged.push('\n');
            }
        }
    }
    Ok(merged)
}

fn resolve_manifest_source_files(manifest_path: &Path) -> Result<Vec<PathBuf>, EvidenceError> {
    let source = std::fs::read_to_string(manifest_path).map_err(|e| {
        EvidenceError::CanonicalizationFailed {
            detail: format!("read manifest {}: {e}", manifest_path.display()),
        }
    })?;
    let root = parse_toml_root(manifest_path, &source)?;
    let mut files = vec![manifest_path.to_path_buf()];
    if !is_schema_v2(&root) {
        return Ok(files);
    }
    let include = root
        .get("include")
        .and_then(TomlValue::as_table)
        .ok_or_else(|| EvidenceError::CanonicalizationFailed {
            detail: format!(
                "manifest {} missing [include] table for schema v2",
                manifest_path.display()
            ),
        })?;
    let root_dir = manifest_path.parent().unwrap_or_else(|| Path::new(""));
    for category in ["phases", "markers", "profiles"] {
        let pattern = include_pattern(include, category, manifest_path)?;
        files.extend(expand_include_glob(
            root_dir,
            category,
            pattern,
            manifest_path,
        )?);
    }
    Ok(files)
}

fn parse_toml_root(path: &Path, source: &str) -> Result<TomlValue, EvidenceError> {
    toml::from_str(source).map_err(|e| EvidenceError::CanonicalizationFailed {
        detail: format!("parse manifest root {}: {e}", path.display()),
    })
}

fn is_schema_v2(root: &TomlValue) -> bool {
    root.get("meta")
        .and_then(TomlValue::as_table)
        .and_then(|m| m.get("schema_version"))
        .and_then(TomlValue::as_str)
        == Some("2")
}

fn include_pattern<'a>(
    include: &'a toml::value::Table,
    category: &'static str,
    manifest_path: &Path,
) -> Result<&'a str, EvidenceError> {
    include
        .get(category)
        .and_then(TomlValue::as_str)
        .ok_or_else(|| EvidenceError::CanonicalizationFailed {
            detail: format!(
                "manifest {} missing [include].{} for schema v2",
                manifest_path.display(),
                category
            ),
        })
}

fn expand_include_glob(
    root_dir: &Path,
    category: &'static str,
    pattern: &str,
    manifest_path: &Path,
) -> Result<Vec<PathBuf>, EvidenceError> {
    let joined = root_dir.join(pattern);
    let joined_str = joined.to_string_lossy().to_string();
    let mut files = Vec::new();
    for entry in glob(&joined_str).map_err(|e| EvidenceError::CanonicalizationFailed {
        detail: format!(
            "expand include glob {} in {}: {e}",
            pattern,
            manifest_path.display()
        ),
    })? {
        match entry {
            Ok(path) => files.push(path),
            Err(e) => {
                return Err(EvidenceError::CanonicalizationFailed {
                    detail: format!(
                        "resolve include {} in {}: {e}",
                        pattern,
                        manifest_path.display()
                    ),
                })
            }
        }
    }
    files.sort();
    if files.is_empty() {
        return Err(EvidenceError::CanonicalizationFailed {
            detail: format!(
                "include glob empty for {} in {}: {}",
                category,
                manifest_path.display(),
                pattern
            ),
        });
    }
    Ok(files)
}

// ---------------------------------------------------------------------------
// Test-only constructors and helpers.
//
// `canonical.rs` is `pub(crate)`; integration tests live in `tests/` and
// can't reach private items directly. We expose a small builder surface
// gated on `#[cfg(any(test, feature = "test-helpers"))]` so future cuts
// can grow it without polluting the main API.
// ---------------------------------------------------------------------------

/// Test-only helpers exposed under `#[cfg(test)]`-equivalent access via
/// the `test-helpers` Cargo feature (none yet for P5-01; integration
/// tests build their fixtures by hand against the public schema).
#[doc(hidden)]
pub mod test_support {
    use super::*;

    /// Construct a minimal, schema-valid empty bundle. Useful as a
    /// starting point for canonical-hash tests. All collections are
    /// empty; all strings are empty; signature is `None`.
    pub fn empty_bundle() -> Bundle {
        Bundle {
            meta: BundleMeta {
                schema_version: 1,
                profile: String::new(),
            },
            manifest: ManifestArtifact { bytes: Vec::new() },
            uart: UartArtifact { bytes: Vec::new() },
            trace: TraceArtifact::default(),
            config: ConfigArtifact {
                profile: String::new(),
                env: BTreeMap::new(),
                kernel_cmdline: String::new(),
                qemu_args: Vec::new(),
                host_info: String::new(),
                build_sha: String::new(),
                rustc_version: String::new(),
                qemu_version: String::new(),
                wall_clock_utc: String::new(),
            },
            signature: None,
        }
    }
}
