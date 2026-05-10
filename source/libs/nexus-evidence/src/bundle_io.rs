// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: On-disk bundle I/O (P5-02). Reads and writes the outer
//! `<utc>-<profile>-<git-sha>.tar.gz` plus the inner `manifest.tar`
//! with strict reproducibility guarantees:
//!
//!   - Outer-tar entries are emitted in **lexicographically sorted**
//!     order (`config.json`, `manifest.tar`, `meta.json`,
//!     `signature.bin`?, `trace.jsonl`, `uart.log`).
//!   - Every entry header uses `mtime=0`, `uid=0`, `gid=0`,
//!     `mode=0o644`, empty `uname`/`gname`. This is what lets two
//!     reseals of the same run produce a byte-identical outer tar
//!     modulo `wall_clock_utc` (see canonical-hash spec).
//!   - The gzip wrapper sets `mtime=0` in its header (`flate2`
//!     `GzBuilder::mtime(0)`).
//!
//! On-tar layout (P5-02 freezes this):
//!   - `meta.json` — bundle-schema version + profile (JSON).
//!   - `manifest.tar` — verbatim manifest (single file for v1, full
//!     split tree for v2; inner tar is itself reproducible).
//!   - `uart.log` — raw bytes.
//!   - `trace.jsonl` — one JSON object per line (UART appearance order).
//!   - `config.json` — full [`ConfigArtifact`] (incl. `wall_clock_utc`).
//!   - `signature.bin` — present only after P5-03 seal.
//!
//! OWNERS: @runtime
//! STATUS: Functional (P5-02 surface; signature support lands in P5-03)
//! API_STABILITY: Unstable (Phase 5 evolves shape between cuts)
//! TEST_COVERAGE: see `tests/assemble.rs` (5 tests)

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use flate2::Compression;
use tar::{Builder, Header};

use crate::{
    key::Signature, Bundle, BundleMeta, ConfigArtifact, EvidenceError, ManifestArtifact,
    TraceArtifact, TraceEntry, UartArtifact,
};

/// Write `bundle` to `path` as an unsigned `tar.gz`. Overwrites any
/// existing file. The bundle's `signature` field is intentionally NOT
/// written by this entry point — sealing is a P5-03 concern.
///
/// The output layout is described in this module's CONTEXT header.
///
/// # Errors
///
/// - [`EvidenceError::CanonicalizationFailed`] if any underlying I/O
///   or serialization step fails. The variant payload carries a stable
///   short diagnostic for grep-based test assertions.
pub fn write_unsigned(bundle: &Bundle, path: &Path) -> Result<(), EvidenceError> {
    let f = File::create(path).map_err(|e| io_err("create_outer_tar_gz", e))?;
    write_unsigned_to_writer(bundle, f)
}

/// Same as [`write_unsigned`] but writes to any [`Write`] sink. Used
/// internally by tests (write to `Vec<u8>`) and by [`canonical_hash`]
/// callers that need the raw bytes.
pub fn write_unsigned_to_writer<W: Write>(bundle: &Bundle, sink: W) -> Result<(), EvidenceError> {
    // gzip wrapper with mtime=0 in the header for reproducibility.
    let gz = flate2::GzBuilder::new().mtime(0).write(sink, Compression::default());
    let mut tar = Builder::new(gz);

    // Sorted entries (lexicographic on filename) for deterministic
    // outer-tar byte layout. `signature.bin` is included only when
    // the bundle is sealed; it sorts between `meta.json` and
    // `trace.jsonl`, preserving lexicographic order.
    let mut entries: Vec<(&str, Vec<u8>)> = vec![
        ("config.json", serialize_config(&bundle.config)?),
        ("manifest.tar", bundle.manifest.bytes.clone()),
        ("meta.json", serialize_meta(&bundle.meta)?),
        ("trace.jsonl", serialize_trace(&bundle.trace)?),
        ("uart.log", bundle.uart.bytes.clone()),
    ];
    if let Some(sig) = &bundle.signature {
        entries.push(("signature.bin", sig.to_bytes().to_vec()));
        entries.sort_by(|a, b| a.0.cmp(b.0));
    }

    for (name, data) in &entries {
        append_entry(&mut tar, name, data)?;
    }

    let gz = tar.into_inner().map_err(|e| io_err("finish_outer_tar", e))?;
    let _ = gz.finish().map_err(|e| io_err("finish_gzip", e))?;
    Ok(())
}

/// Read a bundle from `path`. If the archive contains
/// `signature.bin`, it is parsed and attached to the returned
/// [`Bundle::signature`]; otherwise the field is `None`. The
/// function name is historical (P5-02 only handled the unsigned
/// case); from P5-03 it transparently handles both.
///
/// # Errors
///
/// - [`EvidenceError::MissingArtifact`] if any of the 4 required
///   files (`meta.json`, `manifest.tar`, `uart.log`, `trace.jsonl`,
///   `config.json` — that's 5) is absent.
/// - [`EvidenceError::SchemaVersionUnsupported`] if `meta.json`
///   declares a bundle schema this build doesn't understand.
/// - [`EvidenceError::MalformedConfig`] / [`EvidenceError::MalformedTrace`]
///   on JSON deserialization failure.
pub fn read_unsigned(path: &Path) -> Result<Bundle, EvidenceError> {
    let f = File::open(path).map_err(|e| io_err("open_outer_tar_gz", e))?;
    let mut gz = flate2::read::GzDecoder::new(f);
    let mut decompressed = Vec::new();
    gz.read_to_end(&mut decompressed).map_err(|e| io_err("decompress_gzip", e))?;
    read_unsigned_from_bytes(&decompressed)
}

/// Same as [`read_unsigned`] but operates on already-decompressed tar
/// bytes. Used internally by tests.
pub fn read_unsigned_from_bytes(tar_bytes: &[u8]) -> Result<Bundle, EvidenceError> {
    let mut archive = tar::Archive::new(Cursor::new(tar_bytes));

    let mut meta_json: Option<Vec<u8>> = None;
    let mut manifest_tar: Option<Vec<u8>> = None;
    let mut uart_log: Option<Vec<u8>> = None;
    let mut trace_jsonl: Option<Vec<u8>> = None;
    let mut config_json: Option<Vec<u8>> = None;
    let mut signature_bin: Option<Vec<u8>> = None;

    for entry in archive.entries().map_err(|e| io_err("read_tar_entries", e))? {
        let mut entry = entry.map_err(|e| io_err("read_tar_entry", e))?;
        let name =
            entry.path().map_err(|e| io_err("read_tar_path", e))?.to_string_lossy().into_owned();
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).map_err(|e| io_err("read_tar_entry_body", e))?;
        match name.as_str() {
            "meta.json" => meta_json = Some(buf),
            "manifest.tar" => manifest_tar = Some(buf),
            "uart.log" => uart_log = Some(buf),
            "trace.jsonl" => trace_jsonl = Some(buf),
            "config.json" => config_json = Some(buf),
            "signature.bin" => signature_bin = Some(buf),
            _ => {
                return Err(EvidenceError::CanonicalizationFailed {
                    detail: format!("unexpected_bundle_entry `{}`", name),
                });
            }
        }
    }

    let meta = parse_meta(meta_json.ok_or(missing("meta.json"))?.as_slice())?;
    let manifest = ManifestArtifact { bytes: manifest_tar.ok_or(missing("manifest.tar"))? };
    let uart = UartArtifact { bytes: uart_log.ok_or(missing("uart.log"))? };
    let trace = parse_trace(trace_jsonl.ok_or(missing("trace.jsonl"))?.as_slice())?;
    let config = parse_config(config_json.ok_or(missing("config.json"))?.as_slice())?;

    let signature = match signature_bin {
        Some(bytes) => Some(Signature::from_bytes(&bytes)?),
        None => None,
    };

    Ok(Bundle { meta, manifest, uart, trace, config, signature })
}

// ---------------------------------------------------------------------------
// Serialization helpers (private)
// ---------------------------------------------------------------------------

fn serialize_meta(meta: &BundleMeta) -> Result<Vec<u8>, EvidenceError> {
    serde_json::to_vec(meta).map_err(|e| EvidenceError::CanonicalizationFailed {
        detail: format!("serialize_meta: {}", e),
    })
}

fn serialize_config(config: &ConfigArtifact) -> Result<Vec<u8>, EvidenceError> {
    serde_json::to_vec(config).map_err(|e| EvidenceError::CanonicalizationFailed {
        detail: format!("serialize_config: {}", e),
    })
}

fn serialize_trace(trace: &TraceArtifact) -> Result<Vec<u8>, EvidenceError> {
    let mut out = Vec::new();
    for (idx, entry) in trace.entries.iter().enumerate() {
        if idx > 0 {
            out.push(b'\n');
        }
        let line =
            serde_json::to_vec(entry).map_err(|e| EvidenceError::CanonicalizationFailed {
                detail: format!("serialize_trace_entry: {}", e),
            })?;
        out.extend_from_slice(&line);
    }
    if !trace.entries.is_empty() {
        out.push(b'\n');
    }
    Ok(out)
}

fn parse_meta(bytes: &[u8]) -> Result<BundleMeta, EvidenceError> {
    let meta: BundleMeta = serde_json::from_slice(bytes)
        .map_err(|e| EvidenceError::MalformedConfig { detail: format!("parse_meta: {}", e) })?;
    if meta.schema_version != 1 {
        return Err(EvidenceError::SchemaVersionUnsupported { version: meta.schema_version });
    }
    Ok(meta)
}

fn parse_config(bytes: &[u8]) -> Result<ConfigArtifact, EvidenceError> {
    let config: ConfigArtifact = serde_json::from_slice(bytes)
        .map_err(|e| EvidenceError::MalformedConfig { detail: format!("parse_config: {}", e) })?;
    if config.profile.is_empty() {
        return Err(EvidenceError::MalformedConfig { detail: "empty_profile".into() });
    }
    let _: &BTreeMap<String, String> = &config.env;
    Ok(config)
}

fn parse_trace(bytes: &[u8]) -> Result<TraceArtifact, EvidenceError> {
    let mut entries = Vec::new();
    for (lineno, raw_line) in bytes.split(|&b| b == b'\n').enumerate() {
        if raw_line.is_empty() {
            continue;
        }
        let entry: TraceEntry =
            serde_json::from_slice(raw_line).map_err(|e| EvidenceError::MalformedTrace {
                detail: format!("parse_trace_line {}: {}", lineno + 1, e),
            })?;
        entries.push(entry);
    }
    Ok(TraceArtifact { entries })
}

fn append_entry<W: Write>(
    tar: &mut Builder<W>,
    name: &str,
    data: &[u8],
) -> Result<(), EvidenceError> {
    let mut header = Header::new_gnu();
    header.set_path(name).map_err(|e| io_err("set_tar_path", e))?;
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_uid(0);
    header.set_gid(0);
    header.set_username("").map_err(|e| io_err("set_tar_uname", e))?;
    header.set_groupname("").map_err(|e| io_err("set_tar_gname", e))?;
    header.set_entry_type(tar::EntryType::Regular);
    header.set_cksum();
    tar.append(&header, data).map_err(|e| io_err("append_tar_entry", e))?;
    Ok(())
}

fn missing(name: &'static str) -> EvidenceError {
    EvidenceError::MissingArtifact { artifact: name }
}

fn io_err(label: &str, e: impl std::fmt::Display) -> EvidenceError {
    EvidenceError::CanonicalizationFailed { detail: format!("{}: {}", label, e) }
}

// ---------------------------------------------------------------------------
// Inner-tar packing (manifest tree)
// ---------------------------------------------------------------------------

/// Pack a list of (relative-path, bytes) pairs into a deterministic
/// inner tar (no gzip; the outer tar handles compression). Used by
/// [`crate::Bundle::assemble`] to produce [`ManifestArtifact::bytes`]
/// from a v2 split tree on disk.
///
/// Entries are sorted lexicographically by path before emission;
/// every header uses `mtime=0`, `uid=0`, `gid=0`, `mode=0o644`,
/// empty `uname`/`gname` — matching the outer-tar convention.
pub fn pack_manifest_tree(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, EvidenceError> {
    let mut sorted = files.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    let mut buf = Vec::new();
    {
        let mut tar = Builder::new(&mut buf);
        for (name, data) in &sorted {
            append_entry(&mut tar, name, data)?;
        }
        tar.finish().map_err(|e| io_err("finish_manifest_tar", e))?;
    }
    Ok(buf)
}
